use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use rostfrei_messaging_core::{
    ApplicationErrorCode, BoundedContext, CallerMetadata, CausationId, CorrelationId,
    EnvelopeContext, MessageId, MessageTimestamp, QueryAddress, QueryErrorClassification,
    QueryErrorPayload, QueryHandler, QueryOptions, QueryOutcome,
    QueryRequest as MessageQueryRequest, QueryRequestError, QueryRequestErrorKind, QueryRequester,
    QueryResponse, QueryResult, SchemaVersion, TraceContext,
};
use rostfrei_registry::QueryDefinition;
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned, de::Error as _};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

const INVALID_QUERY_CODE: &str = "rostfrei.query.invalid";
const INVALID_QUERY_PAYLOAD_CODE: &str = "rostfrei.query.invalid-payload";
const UNKNOWN_QUERY_CODE: &str = "rostfrei.query.unknown";

#[derive(Clone, Debug)]
pub struct QueryRequest<Q> {
    query: Q,
    message_id: Option<MessageId>,
    correlation_id: Option<CorrelationId>,
    causation_id: Option<CausationId>,
    created_at: Option<MessageTimestamp>,
    metadata: CallerMetadata,
    trace_context: Option<TraceContext>,
}

impl<Q> QueryRequest<Q> {
    pub fn new(query: Q) -> Self {
        Self {
            query,
            message_id: None,
            correlation_id: None,
            causation_id: None,
            created_at: None,
            metadata: CallerMetadata::default(),
            trace_context: None,
        }
    }

    #[must_use]
    pub fn with_message_id(mut self, message_id: MessageId) -> Self {
        self.message_id = Some(message_id);
        self
    }

    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: CorrelationId) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    #[must_use]
    pub fn with_causation_id(mut self, causation_id: CausationId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    #[must_use]
    pub const fn with_created_at(mut self, created_at: MessageTimestamp) -> Self {
        self.created_at = Some(created_at);
        self
    }

    #[must_use]
    pub fn with_metadata(mut self, metadata: CallerMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    #[must_use]
    pub fn with_trace_context(mut self, trace_context: TraceContext) -> Self {
        self.trace_context = Some(trace_context);
        self
    }
}

#[derive(Clone, Debug)]
pub struct DynamicQueryRequest {
    query: String,
    schema_version: u32,
    payload: Value,
    message_id: Option<MessageId>,
    correlation_id: Option<CorrelationId>,
    causation_id: Option<CausationId>,
    created_at: Option<MessageTimestamp>,
    metadata: CallerMetadata,
    trace_context: Option<TraceContext>,
}

impl DynamicQueryRequest {
    pub fn new(
        query: impl Into<String>,
        schema_version: u32,
        payload: Value,
    ) -> Result<Self, QueryBusError> {
        let query = query.into();
        QueryAddress::new("rostfrei", "dynamic-query", &query)
            .map_err(|error| QueryBusError::encoding(error.to_string()))?;
        SchemaVersion::new(schema_version)
            .map_err(|error| QueryBusError::encoding(error.to_string()))?;
        Ok(Self {
            query,
            schema_version,
            payload,
            message_id: None,
            correlation_id: None,
            causation_id: None,
            created_at: None,
            metadata: CallerMetadata::default(),
            trace_context: None,
        })
    }

    #[must_use]
    pub fn with_message_id(mut self, message_id: MessageId) -> Self {
        self.message_id = Some(message_id);
        self
    }

    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: CorrelationId) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    #[must_use]
    pub fn with_causation_id(mut self, causation_id: CausationId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    #[must_use]
    pub const fn with_created_at(mut self, created_at: MessageTimestamp) -> Self {
        self.created_at = Some(created_at);
        self
    }

    #[must_use]
    pub fn with_metadata(mut self, metadata: CallerMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    #[must_use]
    pub fn with_trace_context(mut self, trace_context: TraceContext) -> Self {
        self.trace_context = Some(trace_context);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RoutedQuery {
    bounded_context: String,
    query: String,
    schema_version: u32,
    payload: Value,
}

impl RoutedQuery {
    pub fn new(
        bounded_context: impl Into<String>,
        query: impl Into<String>,
        schema_version: u32,
        payload: Value,
    ) -> Result<Self, RoutedQueryError> {
        let bounded_context = bounded_context.into();
        let query = query.into();
        QueryAddress::new("rostfrei", &bounded_context, &query)
            .map_err(RoutedQueryError::Identity)?;
        SchemaVersion::new(schema_version).map_err(RoutedQueryError::Identity)?;
        Ok(Self {
            bounded_context,
            query,
            schema_version,
            payload,
        })
    }

    pub fn bounded_context(&self) -> &str {
        &self.bounded_context
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn payload(&self) -> &Value {
        &self.payload
    }
}

#[derive(Deserialize)]
struct RoutedQueryWire {
    bounded_context: String,
    query: String,
    schema_version: u32,
    payload: Value,
}

impl<'de> Deserialize<'de> for RoutedQuery {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RoutedQueryWire::deserialize(deserializer)?;
        Self::new(
            wire.bounded_context,
            wire.query,
            wire.schema_version,
            wire.payload,
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RoutedQueryError {
    #[error("invalid routed query identity: {0}")]
    Identity(rostfrei_messaging_core::ContractError),
}

#[derive(Clone, Debug)]
pub struct EncodedQuery {
    address: QueryAddress,
    request: MessageQueryRequest<RoutedQuery>,
}

impl EncodedQuery {
    pub const fn address(&self) -> &QueryAddress {
        &self.address
    }

    pub const fn request(&self) -> &MessageQueryRequest<RoutedQuery> {
        &self.request
    }

    pub fn into_request(self) -> MessageQueryRequest<RoutedQuery> {
        self.request
    }
}

#[async_trait]
pub trait QueryMessageAdapter: Send + Sync {
    async fn request(&self, query: EncodedQuery, options: QueryOptions) -> QueryResult<Value>;
}

#[async_trait]
impl<T> QueryMessageAdapter for T
where
    T: QueryRequester<RoutedQuery, Value> + Send + Sync,
{
    async fn request(&self, query: EncodedQuery, options: QueryOptions) -> QueryResult<Value> {
        let address = query.address().clone();
        QueryRequester::<RoutedQuery, Value>::request(self, &address, query.into_request(), options)
            .await
    }
}

#[derive(Clone)]
pub struct QueryBus {
    context: BoundedContext,
    adapter: Arc<dyn QueryMessageAdapter>,
}

impl QueryBus {
    pub const fn new(context: BoundedContext, adapter: Arc<dyn QueryMessageAdapter>) -> Self {
        Self { context, adapter }
    }

    pub const fn context(&self) -> &BoundedContext {
        &self.context
    }

    pub async fn request<Q>(
        &self,
        request: QueryRequest<Q>,
        options: QueryOptions,
    ) -> Result<QueryResponse<Q::Response>, QueryBusError>
    where
        Q: QueryDefinition + Serialize,
        Q::Response: DeserializeOwned + Serialize,
    {
        if Q::BOUNDED_CONTEXT != self.context.name().as_str() {
            return Err(QueryBusError::encoding(format!(
                "query `{}` belongs to bounded context `{}`, not `{}`",
                Q::QUERY_NAME,
                Q::BOUNDED_CONTEXT,
                self.context.name().as_str(),
            )));
        }
        let payload = serde_json::to_value(request.query)
            .map_err(|error| QueryBusError::encoding(error.to_string()))?;
        let response = self
            .request_dynamic(
                DynamicQueryRequest {
                    query: Q::QUERY_NAME.to_owned(),
                    schema_version: Q::SCHEMA_VERSION,
                    payload,
                    message_id: request.message_id,
                    correlation_id: request.correlation_id,
                    causation_id: request.causation_id,
                    created_at: request.created_at,
                    metadata: request.metadata,
                    trace_context: request.trace_context,
                },
                options,
            )
            .await?;
        decode_query_response(response)
    }

    pub async fn request_dynamic(
        &self,
        request: DynamicQueryRequest,
        options: QueryOptions,
    ) -> Result<QueryResponse<Value>, QueryBusError> {
        let encoded = self.encode_dynamic(request)?;
        let expected_request_id = encoded.request().message_id().clone();
        let expected_schema_version = encoded.request().schema_version();
        let expected_correlation_id = encoded.request().correlation_id().clone();
        let response = self
            .adapter
            .request(encoded, options)
            .await
            .map_err(QueryBusError::request)?;
        if response.request_id() != &expected_request_id
            || response.schema_version() != expected_schema_version
            || response.correlation_id() != &expected_correlation_id
        {
            return Err(QueryBusError::new(
                QueryBusErrorKind::InvalidResponse,
                "query response identity does not match the request",
            ));
        }
        Ok(response)
    }

    pub fn encode_dynamic(
        &self,
        request: DynamicQueryRequest,
    ) -> Result<EncodedQuery, QueryBusError> {
        let address = self
            .context
            .query_address(&request.query)
            .map_err(|error| QueryBusError::encoding(error.to_string()))?;
        let created_at = request.created_at.map_or_else(current_timestamp, Ok)?;
        let message_id = request.message_id.map_or_else(next_query_request_id, Ok)?;
        let correlation_id = request.correlation_id.map_or_else(
            || {
                CorrelationId::new(message_id.as_str())
                    .map_err(|error| QueryBusError::encoding(error.to_string()))
            },
            Ok,
        )?;
        let schema_version = SchemaVersion::new(request.schema_version)
            .map_err(|error| QueryBusError::encoding(error.to_string()))?;
        let routed = RoutedQuery::new(
            self.context.name().as_str(),
            request.query,
            request.schema_version,
            request.payload,
        )
        .map_err(|error| QueryBusError::encoding(error.to_string()))?;
        let request = MessageQueryRequest::new(
            EnvelopeContext::new(
                message_id,
                schema_version,
                correlation_id,
                request.causation_id,
            ),
            created_at,
            request.metadata,
            request.trace_context,
            routed,
        )
        .map_err(|error| QueryBusError::encoding(error.to_string()))?;
        Ok(EncodedQuery { address, request })
    }
}

fn decode_query_response<Response>(
    response: QueryResponse<Value>,
) -> Result<QueryResponse<Response>, QueryBusError>
where
    Response: DeserializeOwned + Serialize,
{
    let request_id = response.request_id().clone();
    let schema_version = response.schema_version();
    let responded_at = response.responded_at();
    let correlation_id = response.correlation_id().clone();
    let trace_context = response.trace_context().cloned();
    match response.into_outcome() {
        QueryOutcome::Success(payload) => {
            let payload = serde_json::from_value(payload).map_err(|error| {
                QueryBusError::new(QueryBusErrorKind::InvalidResponse, error.to_string())
            })?;
            QueryResponse::success(
                request_id,
                schema_version,
                responded_at,
                correlation_id,
                trace_context,
                payload,
            )
        }
        QueryOutcome::Error(error) => QueryResponse::error(
            request_id,
            schema_version,
            responded_at,
            correlation_id,
            trace_context,
            error,
        ),
    }
    .map_err(|error| QueryBusError::new(QueryBusErrorKind::InvalidResponse, error.to_string()))
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum QueryBusErrorKind {
    #[error("query encoding failed")]
    Encoding,
    #[error("query request timed out")]
    Timeout,
    #[error("query request was rejected")]
    Rejected,
    #[error("query messaging is unavailable")]
    Unavailable,
    #[error("query response is invalid")]
    InvalidResponse,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{message}")]
pub struct QueryBusError {
    kind: QueryBusErrorKind,
    message: String,
}

impl QueryBusError {
    pub fn new(kind: QueryBusErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn encoding(message: impl Into<String>) -> Self {
        Self::new(QueryBusErrorKind::Encoding, message)
    }

    fn request(error: QueryRequestError) -> Self {
        let kind = match error.kind() {
            QueryRequestErrorKind::Serialization => QueryBusErrorKind::Encoding,
            QueryRequestErrorKind::Timeout => QueryBusErrorKind::Timeout,
            QueryRequestErrorKind::Unavailable => QueryBusErrorKind::Unavailable,
            QueryRequestErrorKind::Rejected => QueryBusErrorKind::Rejected,
            QueryRequestErrorKind::ResponseTooLarge | QueryRequestErrorKind::InvalidResponse => {
                QueryBusErrorKind::InvalidResponse
            }
            _ => QueryBusErrorKind::InvalidResponse,
        };
        Self::new(kind, error.to_string())
    }

    pub const fn kind(&self) -> QueryBusErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct QueryBindingKey {
    bounded_context: String,
    query: String,
    schema_version: u32,
}

impl QueryBindingKey {
    fn new(bounded_context: &str, query: &str, schema_version: u32) -> Self {
        Self {
            bounded_context: bounded_context.to_owned(),
            query: query.to_owned(),
            schema_version,
        }
    }
}

#[async_trait]
trait ErasedQueryBinding: Send + Sync {
    async fn handle(&self, request: MessageQueryRequest<Value>)
    -> Result<Value, QueryErrorPayload>;
}

struct TypedQueryBinding<Q>
where
    Q: QueryDefinition,
{
    handler: Arc<dyn QueryHandler<Q, Q::Response>>,
}

#[async_trait]
impl<Q> ErasedQueryBinding for TypedQueryBinding<Q>
where
    Q: QueryDefinition + DeserializeOwned + Serialize,
    Q::Response: Serialize,
{
    async fn handle(
        &self,
        request: MessageQueryRequest<Value>,
    ) -> Result<Value, QueryErrorPayload> {
        let query = serde_json::from_value::<Q>(request.payload().clone()).map_err(|_| {
            framework_query_error(
                QueryErrorClassification::InvalidRequest,
                INVALID_QUERY_PAYLOAD_CODE,
                "The query payload is invalid.",
            )
        })?;
        let typed_request = MessageQueryRequest::new(
            EnvelopeContext::new(
                request.message_id().clone(),
                request.schema_version(),
                request.correlation_id().clone(),
                request.causation_id().cloned(),
            ),
            request.created_at(),
            request.metadata().clone(),
            request.trace_context().cloned(),
            query,
        )
        .map_err(|_| QueryErrorPayload::internal_error())?;
        let response = self.handler.handle(typed_request).await?;
        serde_json::to_value(response).map_err(|_| QueryErrorPayload::internal_error())
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum QueryBindingRegistrationError {
    #[error("query definition is invalid: {message}")]
    InvalidDefinition { message: String },
    #[error(
        "query `{query}` version {schema_version} for bounded context `{bounded_context}` is already bound"
    )]
    Duplicate {
        bounded_context: &'static str,
        query: &'static str,
        schema_version: u32,
    },
}

#[derive(Default)]
pub struct QueryProcessor {
    bindings: HashMap<QueryBindingKey, Arc<dyn ErasedQueryBinding>>,
}

impl QueryProcessor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<Q>(
        &mut self,
        handler: Arc<dyn QueryHandler<Q, Q::Response>>,
    ) -> Result<&mut Self, QueryBindingRegistrationError>
    where
        Q: QueryDefinition + DeserializeOwned + Serialize,
        Q::Response: Serialize,
    {
        QueryAddress::new("rostfrei", Q::BOUNDED_CONTEXT, Q::QUERY_NAME)
            .and_then(|_| SchemaVersion::new(Q::SCHEMA_VERSION).map(|_| ()))
            .map_err(|error| QueryBindingRegistrationError::InvalidDefinition {
                message: error.to_string(),
            })?;
        let key = QueryBindingKey::new(Q::BOUNDED_CONTEXT, Q::QUERY_NAME, Q::SCHEMA_VERSION);
        if self.bindings.contains_key(&key) {
            return Err(QueryBindingRegistrationError::Duplicate {
                bounded_context: Q::BOUNDED_CONTEXT,
                query: Q::QUERY_NAME,
                schema_version: Q::SCHEMA_VERSION,
            });
        }
        self.bindings
            .insert(key, Arc::new(TypedQueryBinding::<Q> { handler }));
        Ok(self)
    }

    pub fn handler(self: &Arc<Self>, address: QueryAddress) -> QueryProcessorHandler {
        QueryProcessorHandler {
            processor: Arc::clone(self),
            address,
        }
    }

    pub async fn process(
        &self,
        address: &QueryAddress,
        request: MessageQueryRequest<RoutedQuery>,
    ) -> Result<Value, QueryErrorPayload> {
        let routed = request.payload();
        if address.context() != routed.bounded_context()
            || address.name() != routed.query()
            || request.schema_version().get() != routed.schema_version()
        {
            return Err(framework_query_error(
                QueryErrorClassification::InvalidRequest,
                INVALID_QUERY_CODE,
                "The query envelope and route are inconsistent.",
            ));
        }
        let key = QueryBindingKey::new(
            routed.bounded_context(),
            routed.query(),
            routed.schema_version(),
        );
        let Some(binding) = self.bindings.get(&key) else {
            return Err(framework_query_error(
                QueryErrorClassification::InvalidRequest,
                UNKNOWN_QUERY_CODE,
                "The query name or schema version is not registered.",
            ));
        };
        let payload_request = MessageQueryRequest::new(
            EnvelopeContext::new(
                request.message_id().clone(),
                request.schema_version(),
                request.correlation_id().clone(),
                request.causation_id().cloned(),
            ),
            request.created_at(),
            request.metadata().clone(),
            request.trace_context().cloned(),
            routed.payload().clone(),
        )
        .map_err(|_| QueryErrorPayload::internal_error())?;
        binding.handle(payload_request).await
    }
}

pub struct QueryProcessorHandler {
    processor: Arc<QueryProcessor>,
    address: QueryAddress,
}

#[async_trait]
impl QueryHandler<RoutedQuery, Value> for QueryProcessorHandler {
    async fn handle(
        &self,
        request: MessageQueryRequest<RoutedQuery>,
    ) -> Result<Value, QueryErrorPayload> {
        self.processor.process(&self.address, request).await
    }
}

pub struct InMemoryQueryAdapter {
    processor: Arc<QueryProcessor>,
}

impl InMemoryQueryAdapter {
    pub const fn new(processor: Arc<QueryProcessor>) -> Self {
        Self { processor }
    }
}

#[async_trait]
impl QueryRequester<RoutedQuery, Value> for InMemoryQueryAdapter {
    async fn request(
        &self,
        address: &QueryAddress,
        request: MessageQueryRequest<RoutedQuery>,
        options: QueryOptions,
    ) -> QueryResult<Value> {
        let request_id = request.message_id().clone();
        let schema_version = request.schema_version();
        let correlation_id = request.correlation_id().clone();
        let trace_context = request.trace_context().cloned();
        let outcome =
            tokio::time::timeout(options.timeout(), self.processor.process(address, request))
                .await
                .map_err(|_| QueryRequestError::new(QueryRequestErrorKind::Timeout))?;
        let responded_at = current_timestamp()
            .map_err(|_| QueryRequestError::new(QueryRequestErrorKind::InvalidResponse))?;
        let response = match outcome {
            Ok(payload) => QueryResponse::success(
                request_id,
                schema_version,
                responded_at,
                correlation_id,
                trace_context,
                payload,
            ),
            Err(error) => QueryResponse::error(
                request_id,
                schema_version,
                responded_at,
                correlation_id,
                trace_context,
                error,
            ),
        }
        .map_err(|_| QueryRequestError::new(QueryRequestErrorKind::InvalidResponse))?;
        let encoded = serde_json::to_vec(&response)
            .map_err(|_| QueryRequestError::new(QueryRequestErrorKind::InvalidResponse))?;
        if encoded.len() > options.maximum_response_bytes() {
            return Err(QueryRequestError::new(
                QueryRequestErrorKind::ResponseTooLarge,
            ));
        }
        Ok(response)
    }
}

fn framework_query_error(
    classification: QueryErrorClassification,
    code: &'static str,
    message: &'static str,
) -> QueryErrorPayload {
    ApplicationErrorCode::new(code)
        .and_then(|code| QueryErrorPayload::new(classification, code, message))
        .unwrap_or_else(|_| QueryErrorPayload::internal_error())
}

fn next_query_request_id() -> Result<MessageId, QueryBusError> {
    MessageId::new(format!("query-{}", Uuid::now_v7()))
        .map_err(|error| QueryBusError::encoding(error.to_string()))
}

fn current_timestamp() -> Result<MessageTimestamp, QueryBusError> {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| QueryBusError::encoding("system clock is before the Unix epoch"))?
        .as_millis();
    let milliseconds = u64::try_from(milliseconds).map_err(|_| {
        QueryBusError::encoding("system clock is outside the message timestamp range")
    })?;
    MessageTimestamp::from_unix_milliseconds(milliseconds)
        .map_err(|error| QueryBusError::encoding(error.to_string()))
}
