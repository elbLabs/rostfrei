use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_nats::{
    Client, HeaderMap, Request as NatsRequest, RequestErrorKind as NatsRequestErrorKind,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{de::DeserializeOwned, Serialize};
use tokio::time::timeout;
use zeitstrahl_messaging_core::{
    ApplicationErrorCode, CallerMetadata, CorrelationId, MessageId, MessageTimestamp, QueryAddress,
    QueryErrorClassification, QueryErrorPayload, QueryHandler, QueryOptions, QueryRequest,
    QueryRequestError, QueryRequestErrorKind, QueryRequester, QueryResponse, QueryServer,
    QueryServerError, QueryServerErrorKind, SchemaVersion, TraceContext, MAX_CONCURRENCY,
    MAX_ENVELOPE_BYTES, MAX_PROCESSING_TIMEOUT,
};

use crate::{
    error::NatsError,
    messaging_config::QueueGroup,
    publish::{safe_headers, CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE},
};

pub const REQUEST_ID_HEADER: &str = "Zeitstrahl-Control-Request-Id";
pub const CORRELATION_ID_HEADER: &str = "Zeitstrahl-Control-Correlation-Id";
pub const DEFAULT_QUERY_HANDLER_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_QUERY_SERVER_CONCURRENCY: usize = 64;

#[derive(Clone)]
pub struct NatsQueryRequester {
    client: Client,
}

impl NatsQueryRequester {
    pub const fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl<Request, Response> QueryRequester<Request, Response> for NatsQueryRequester
where
    Request: Serialize + Send + 'static,
    Response: DeserializeOwned + Serialize + Send + 'static,
{
    async fn request(
        &self,
        address: &QueryAddress,
        request: QueryRequest<Request>,
        options: QueryOptions,
    ) -> Result<QueryResponse<Response>, QueryRequestError> {
        let request_id = request.message_id().clone();
        let correlation_id = request.correlation_id().clone();
        let payload = serde_json::to_vec(&request)
            .map_err(|_| QueryRequestError::new(QueryRequestErrorKind::Serialization))?;
        let mut headers = safe_headers(request.metadata(), request.trace_context());
        insert_query_controls(&mut headers, request_id.as_str(), correlation_id.as_str());
        let nats_request = NatsRequest::new()
            .payload(payload.into())
            .headers(headers)
            .timeout(Some(options.timeout()));
        let response = self
            .client
            .send_request(address.as_str().to_owned(), nats_request)
            .await
            .map_err(|error| map_request_error(&error))?;

        if response.payload.len() > options.maximum_response_bytes() {
            return Err(QueryRequestError::new(
                QueryRequestErrorKind::ResponseTooLarge,
            ));
        }
        let headers = response
            .headers
            .as_ref()
            .ok_or_else(|| QueryRequestError::new(QueryRequestErrorKind::InvalidResponse))?;
        validate_response_headers(headers, &request_id, &correlation_id)?;
        let response: QueryResponse<Response> = serde_json::from_slice(&response.payload)
            .map_err(|_| QueryRequestError::new(QueryRequestErrorKind::InvalidResponse))?;
        if response.request_id() != &request_id || response.correlation_id() != &correlation_id {
            return Err(QueryRequestError::new(
                QueryRequestErrorKind::InvalidResponse,
            ));
        }
        Ok(response)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NatsQueryServerConfig {
    queue_group: Option<QueueGroup>,
    handler_timeout: Duration,
    concurrency: usize,
    maximum_request_bytes: usize,
    maximum_response_bytes: usize,
}

impl Default for NatsQueryServerConfig {
    fn default() -> Self {
        Self {
            queue_group: None,
            handler_timeout: DEFAULT_QUERY_HANDLER_TIMEOUT,
            concurrency: DEFAULT_QUERY_SERVER_CONCURRENCY,
            maximum_request_bytes: MAX_ENVELOPE_BYTES,
            maximum_response_bytes: MAX_ENVELOPE_BYTES,
        }
    }
}

impl NatsQueryServerConfig {
    pub fn new(handler_timeout: Duration, concurrency: usize) -> Result<Self, NatsError> {
        let config = Self {
            handler_timeout,
            concurrency,
            ..Self::default()
        };
        config.validate()?;
        Ok(config)
    }

    #[must_use]
    pub fn with_queue_group(mut self, queue_group: QueueGroup) -> Self {
        self.queue_group = Some(queue_group);
        self
    }

    pub fn with_maximum_request_bytes(
        mut self,
        maximum_request_bytes: usize,
    ) -> Result<Self, NatsError> {
        self.maximum_request_bytes = maximum_request_bytes;
        self.validate()?;
        Ok(self)
    }

    pub fn with_maximum_response_bytes(
        mut self,
        maximum_response_bytes: usize,
    ) -> Result<Self, NatsError> {
        self.maximum_response_bytes = maximum_response_bytes;
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), NatsError> {
        if self.handler_timeout.is_zero()
            || self.handler_timeout > MAX_PROCESSING_TIMEOUT
            || self.concurrency == 0
            || self.concurrency > MAX_CONCURRENCY
            || self.maximum_request_bytes == 0
            || self.maximum_request_bytes > MAX_ENVELOPE_BYTES
            || self.maximum_response_bytes == 0
            || self.maximum_response_bytes > MAX_ENVELOPE_BYTES
        {
            return Err(NatsError::Configuration);
        }
        Ok(())
    }

    pub const fn queue_group(&self) -> Option<&QueueGroup> {
        self.queue_group.as_ref()
    }

    pub const fn handler_timeout(&self) -> Duration {
        self.handler_timeout
    }

    pub const fn concurrency(&self) -> usize {
        self.concurrency
    }
}

#[derive(Clone)]
pub struct NatsQueryServer {
    client: Client,
    config: NatsQueryServerConfig,
}

impl NatsQueryServer {
    pub fn new(client: Client, config: NatsQueryServerConfig) -> Result<Self, NatsError> {
        config.validate()?;
        Ok(Self { client, config })
    }
}

#[async_trait]
impl<Request, Response> QueryServer<Request, Response> for NatsQueryServer
where
    Request: DeserializeOwned + Serialize + Send + 'static,
    Response: Serialize + Send + 'static,
{
    async fn run(
        &self,
        address: QueryAddress,
        handler: Arc<dyn QueryHandler<Request, Response>>,
    ) -> Result<(), QueryServerError> {
        let mut subscriber = if let Some(queue_group) = self.config.queue_group() {
            self.client
                .queue_subscribe(address.as_str().to_owned(), queue_group.as_str().to_owned())
                .await
        } else {
            self.client.subscribe(address.as_str().to_owned()).await
        }
        .map_err(|_| QueryServerError::new(QueryServerErrorKind::Unavailable))?;

        subscriber
            .by_ref()
            .for_each_concurrent(self.config.concurrency(), |message| {
                let client = self.client.clone();
                let config = self.config.clone();
                let handler = handler.clone();
                async move {
                    if let Err(error) = process_request(&client, &config, handler, message).await {
                        tracing::warn!(kind = ?error.kind(), "NATS query request was not processed");
                    }
                }
            })
            .await;
        Err(QueryServerError::new(QueryServerErrorKind::Ended))
    }
}

async fn process_request<Request, Response>(
    client: &Client,
    config: &NatsQueryServerConfig,
    handler: Arc<dyn QueryHandler<Request, Response>>,
    message: async_nats::Message,
) -> Result<(), NatsError>
where
    Request: DeserializeOwned + Serialize + Send + 'static,
    Response: Serialize + Send + 'static,
{
    if message.payload.len() > config.maximum_request_bytes {
        return Err(NatsError::PayloadTooLarge {
            actual: message.payload.len(),
            maximum: config.maximum_request_bytes,
        });
    }
    let reply = message.reply.ok_or(NatsError::InvalidMessage)?;
    if !valid_inbox(reply.as_str()) {
        return Err(NatsError::InvalidMessage);
    }
    let headers = message.headers.as_ref().ok_or(NatsError::InvalidMessage)?;
    if one_header(headers, CONTENT_TYPE_HEADER)? != JSON_CONTENT_TYPE {
        return Err(NatsError::InvalidMessage);
    }
    let header_request_id = one_header(headers, REQUEST_ID_HEADER)?;
    let header_correlation_id = one_header(headers, CORRELATION_ID_HEADER)?;
    let request: QueryRequest<Request> =
        serde_json::from_slice(&message.payload).map_err(|_| NatsError::InvalidMessage)?;
    if header_request_id != request.message_id().as_str()
        || header_correlation_id != request.correlation_id().as_str()
    {
        return Err(NatsError::InvalidMessage);
    }

    let request_id = request.message_id().clone();
    let schema_version = request.schema_version();
    let correlation_id = request.correlation_id().clone();
    let trace_context = request.trace_context().cloned();
    let handler_result = timeout(config.handler_timeout, handler.handle(request)).await;
    let response = match handler_result {
        Ok(Ok(payload)) => QueryResponse::success(
            request_id.clone(),
            schema_version,
            now_timestamp()?,
            correlation_id.clone(),
            trace_context.clone(),
            payload,
        ),
        Ok(Err(error)) => QueryResponse::error(
            request_id.clone(),
            schema_version,
            now_timestamp()?,
            correlation_id.clone(),
            trace_context.clone(),
            error,
        ),
        Err(_) => QueryResponse::error(
            request_id.clone(),
            schema_version,
            now_timestamp()?,
            correlation_id.clone(),
            trace_context.clone(),
            adapter_query_error(
                QueryErrorClassification::Timeout,
                "query.handler_timeout",
                "query handler timed out",
            )?,
        ),
    };

    let payload = match response {
        Ok(response) => serde_json::to_vec(&response).map_err(|_| NatsError::Serialization)?,
        Err(_) => fallback_response(
            request_id.clone(),
            schema_version,
            correlation_id.clone(),
            trace_context.clone(),
        )?,
    };
    let payload = if payload.len() <= config.maximum_response_bytes {
        payload
    } else {
        fallback_response(
            request_id.clone(),
            schema_version,
            correlation_id.clone(),
            trace_context.clone(),
        )?
    };
    if payload.len() > config.maximum_response_bytes {
        return Err(NatsError::PayloadTooLarge {
            actual: payload.len(),
            maximum: config.maximum_response_bytes,
        });
    }

    let mut response_headers = safe_headers(&CallerMetadata::default(), trace_context.as_ref());
    insert_query_controls(
        &mut response_headers,
        request_id.as_str(),
        correlation_id.as_str(),
    );
    timeout(
        config.handler_timeout,
        client.publish_with_headers(reply, response_headers, payload.into()),
    )
    .await
    .map_err(|_| NatsError::QueryTimeout)?
    .map_err(|_| NatsError::Query)
}

fn fallback_response(
    request_id: MessageId,
    schema_version: SchemaVersion,
    correlation_id: CorrelationId,
    trace_context: Option<TraceContext>,
) -> Result<Vec<u8>, NatsError> {
    let response = QueryResponse::<serde_json::Value>::error(
        request_id,
        schema_version,
        now_timestamp()?,
        correlation_id,
        trace_context,
        adapter_query_error(
            QueryErrorClassification::Internal,
            "query.response_serialization",
            "query response could not be serialized",
        )?,
    )
    .map_err(|_| NatsError::Serialization)?;
    serde_json::to_vec(&response).map_err(|_| NatsError::Serialization)
}

fn adapter_query_error(
    classification: QueryErrorClassification,
    code: &str,
    message: &str,
) -> Result<QueryErrorPayload, NatsError> {
    let code = ApplicationErrorCode::new(code).map_err(|_| NatsError::Configuration)?;
    QueryErrorPayload::new(classification, code, message).map_err(|_| NatsError::Configuration)
}

fn now_timestamp() -> Result<MessageTimestamp, NatsError> {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| NatsError::Query)?
        .as_millis();
    let milliseconds = u64::try_from(milliseconds).map_err(|_| NatsError::Query)?;
    MessageTimestamp::from_unix_milliseconds(milliseconds).map_err(|_| NatsError::Query)
}

fn insert_query_controls(headers: &mut HeaderMap, request_id: &str, correlation_id: &str) {
    headers.insert(CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE);
    headers.insert(REQUEST_ID_HEADER, request_id);
    headers.insert(CORRELATION_ID_HEADER, correlation_id);
}

fn validate_response_headers(
    headers: &HeaderMap,
    request_id: &MessageId,
    correlation_id: &CorrelationId,
) -> Result<(), QueryRequestError> {
    let invalid = one_header(headers, CONTENT_TYPE_HEADER).ok() != Some(JSON_CONTENT_TYPE)
        || one_header(headers, REQUEST_ID_HEADER).ok() != Some(request_id.as_str())
        || one_header(headers, CORRELATION_ID_HEADER).ok() != Some(correlation_id.as_str());
    if invalid {
        return Err(QueryRequestError::new(
            QueryRequestErrorKind::InvalidResponse,
        ));
    }
    Ok(())
}

fn one_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, NatsError> {
    let mut values = headers.get_all(name.to_owned());
    let first = values.next().ok_or(NatsError::InvalidMessage)?;
    if values.next().is_some() {
        return Err(NatsError::InvalidMessage);
    }
    Ok(first.as_str())
}

fn valid_inbox(value: &str) -> bool {
    value.strip_prefix("_INBOX.").is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix.len() <= 512
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    })
}

fn map_request_error(error: &async_nats::RequestError) -> QueryRequestError {
    let kind = match error.kind() {
        NatsRequestErrorKind::TimedOut => QueryRequestErrorKind::Timeout,
        NatsRequestErrorKind::MaxPayloadExceeded | NatsRequestErrorKind::InvalidSubject => {
            QueryRequestErrorKind::Rejected
        }
        NatsRequestErrorKind::NoResponders | NatsRequestErrorKind::Other => {
            QueryRequestErrorKind::Unavailable
        }
    };
    QueryRequestError::new(kind)
}
