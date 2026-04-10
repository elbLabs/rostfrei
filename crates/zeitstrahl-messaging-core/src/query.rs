use std::{fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    envelope::validate_serialized_size, CallerMetadata, CausationId, ContractError,
    ContractErrorKind, CorrelationId, EnvelopeContext, MessageBuildError, MessageId,
    MessageTimestamp, QueryAddress, QueryRequestError, QueryServerError, SchemaVersion,
    TraceContext, MAX_ENVELOPE_BYTES,
};

pub const MAX_APPLICATION_ERROR_CODE_BYTES: usize = 128;
pub const MAX_QUERY_ERROR_MESSAGE_BYTES: usize = 1024;
pub const MAX_QUERY_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QueryRequest<T> {
    message_id: MessageId,
    schema_version: SchemaVersion,
    created_at: MessageTimestamp,
    correlation_id: CorrelationId,
    #[serde(skip_serializing_if = "Option::is_none")]
    causation_id: Option<CausationId>,
    metadata: CallerMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_context: Option<TraceContext>,
    payload: T,
}

impl<T> QueryRequest<T>
where
    T: Serialize,
{
    pub fn new(
        context: EnvelopeContext,
        created_at: MessageTimestamp,
        metadata: CallerMetadata,
        trace_context: Option<TraceContext>,
        payload: T,
    ) -> Result<Self, MessageBuildError> {
        let (message_id, schema_version, correlation_id, causation_id) = context.into_parts();
        let request = Self {
            message_id,
            schema_version,
            created_at,
            correlation_id,
            causation_id,
            metadata,
            trace_context,
            payload,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), MessageBuildError> {
        validate_serialized_size(self)
    }
}

impl<T> QueryRequest<T> {
    pub const fn message_id(&self) -> &MessageId {
        &self.message_id
    }

    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    pub const fn created_at(&self) -> MessageTimestamp {
        self.created_at
    }

    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    pub const fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }

    pub const fn metadata(&self) -> &CallerMetadata {
        &self.metadata
    }

    pub const fn trace_context(&self) -> Option<&TraceContext> {
        self.trace_context.as_ref()
    }

    pub const fn payload(&self) -> &T {
        &self.payload
    }

    pub fn into_payload(self) -> T {
        self.payload
    }
}

#[derive(Deserialize)]
struct QueryRequestWire<T> {
    message_id: MessageId,
    schema_version: SchemaVersion,
    created_at: MessageTimestamp,
    correlation_id: CorrelationId,
    causation_id: Option<CausationId>,
    metadata: CallerMetadata,
    trace_context: Option<TraceContext>,
    payload: T,
}

impl<'de, T> Deserialize<'de> for QueryRequest<T>
where
    T: Deserialize<'de> + Serialize,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = QueryRequestWire::deserialize(deserializer)?;
        Self::new(
            EnvelopeContext::new(
                wire.message_id,
                wire.schema_version,
                wire.correlation_id,
                wire.causation_id,
            ),
            wire.created_at,
            wire.metadata,
            wire.trace_context,
            wire.payload,
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum QueryErrorClassification {
    InvalidRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    RateLimited,
    Unavailable,
    Timeout,
    Internal,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ApplicationErrorCode(String);

impl ApplicationErrorCode {
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ContractError::new(
                ContractErrorKind::Empty,
                "application error code",
            ));
        }
        if value.len() > MAX_APPLICATION_ERROR_CODE_BYTES {
            return Err(ContractError::bounded(
                ContractErrorKind::TooLong,
                "application error code",
                value.len(),
                MAX_APPLICATION_ERROR_CODE_BYTES,
            ));
        }
        let starts_and_ends_with_alphanumeric = value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && value
                .bytes()
                .next_back()
                .is_some_and(|byte| byte.is_ascii_alphanumeric());
        if !starts_and_ends_with_alphanumeric
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(ContractError::new(
                ContractErrorKind::InvalidFormat,
                "application error code",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ApplicationErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ApplicationErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ApplicationErrorCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QueryErrorPayload {
    classification: QueryErrorClassification,
    code: ApplicationErrorCode,
    message: String,
}

impl QueryErrorPayload {
    pub fn new(
        classification: QueryErrorClassification,
        code: ApplicationErrorCode,
        message: impl Into<String>,
    ) -> Result<Self, ContractError> {
        let message = message.into();
        if message.is_empty() {
            return Err(ContractError::new(
                ContractErrorKind::Empty,
                "query error message",
            ));
        }
        if message.len() > MAX_QUERY_ERROR_MESSAGE_BYTES {
            return Err(ContractError::bounded(
                ContractErrorKind::TooLong,
                "query error message",
                message.len(),
                MAX_QUERY_ERROR_MESSAGE_BYTES,
            ));
        }
        if message.chars().any(char::is_control) {
            return Err(ContractError::new(
                ContractErrorKind::ControlCharacter,
                "query error message",
            ));
        }
        Ok(Self {
            classification,
            code,
            message,
        })
    }

    pub const fn classification(&self) -> QueryErrorClassification {
        self.classification
    }

    pub const fn code(&self) -> &ApplicationErrorCode {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Deserialize)]
struct QueryErrorPayloadWire {
    classification: QueryErrorClassification,
    code: ApplicationErrorCode,
    message: String,
}

impl<'de> Deserialize<'de> for QueryErrorPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = QueryErrorPayloadWire::deserialize(deserializer)?;
        Self::new(wire.classification, wire.code, wire.message).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
pub enum QueryOutcome<T> {
    Success(T),
    Error(QueryErrorPayload),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QueryResponse<T> {
    request_id: MessageId,
    schema_version: SchemaVersion,
    responded_at: MessageTimestamp,
    correlation_id: CorrelationId,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_context: Option<TraceContext>,
    outcome: QueryOutcome<T>,
}

impl<T> QueryResponse<T>
where
    T: Serialize,
{
    pub fn success(
        request_id: MessageId,
        schema_version: SchemaVersion,
        responded_at: MessageTimestamp,
        correlation_id: CorrelationId,
        trace_context: Option<TraceContext>,
        payload: T,
    ) -> Result<Self, MessageBuildError> {
        Self::new(
            request_id,
            schema_version,
            responded_at,
            correlation_id,
            trace_context,
            QueryOutcome::Success(payload),
        )
    }

    pub fn error(
        request_id: MessageId,
        schema_version: SchemaVersion,
        responded_at: MessageTimestamp,
        correlation_id: CorrelationId,
        trace_context: Option<TraceContext>,
        error: QueryErrorPayload,
    ) -> Result<Self, MessageBuildError> {
        Self::new(
            request_id,
            schema_version,
            responded_at,
            correlation_id,
            trace_context,
            QueryOutcome::Error(error),
        )
    }

    fn new(
        request_id: MessageId,
        schema_version: SchemaVersion,
        responded_at: MessageTimestamp,
        correlation_id: CorrelationId,
        trace_context: Option<TraceContext>,
        outcome: QueryOutcome<T>,
    ) -> Result<Self, MessageBuildError> {
        let response = Self {
            request_id,
            schema_version,
            responded_at,
            correlation_id,
            trace_context,
            outcome,
        };
        response.validate()?;
        Ok(response)
    }

    pub fn validate(&self) -> Result<(), MessageBuildError> {
        validate_serialized_size(self)
    }
}

impl<T> QueryResponse<T> {
    pub const fn request_id(&self) -> &MessageId {
        &self.request_id
    }

    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    pub const fn responded_at(&self) -> MessageTimestamp {
        self.responded_at
    }

    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    pub const fn trace_context(&self) -> Option<&TraceContext> {
        self.trace_context.as_ref()
    }

    pub const fn outcome(&self) -> &QueryOutcome<T> {
        &self.outcome
    }

    pub fn into_outcome(self) -> QueryOutcome<T> {
        self.outcome
    }
}

#[derive(Deserialize)]
struct QueryResponseWire<T> {
    request_id: MessageId,
    schema_version: SchemaVersion,
    responded_at: MessageTimestamp,
    correlation_id: CorrelationId,
    trace_context: Option<TraceContext>,
    outcome: QueryOutcome<T>,
}

impl<'de, T> Deserialize<'de> for QueryResponse<T>
where
    T: Deserialize<'de> + Serialize,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = QueryResponseWire::deserialize(deserializer)?;
        Self::new(
            wire.request_id,
            wire.schema_version,
            wire.responded_at,
            wire.correlation_id,
            wire.trace_context,
            wire.outcome,
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryOptions {
    timeout: Duration,
    maximum_response_bytes: usize,
}

impl QueryOptions {
    pub fn new(timeout: Duration, maximum_response_bytes: usize) -> Result<Self, ContractError> {
        if timeout.is_zero() || timeout > MAX_QUERY_TIMEOUT {
            return Err(ContractError::new(
                ContractErrorKind::OutOfRange,
                "query timeout",
            ));
        }
        if maximum_response_bytes == 0 || maximum_response_bytes > MAX_ENVELOPE_BYTES {
            return Err(ContractError::bounded(
                ContractErrorKind::OutOfRange,
                "maximum query response bytes",
                maximum_response_bytes,
                MAX_ENVELOPE_BYTES,
            ));
        }
        Ok(Self {
            timeout,
            maximum_response_bytes,
        })
    }

    pub const fn timeout(self) -> Duration {
        self.timeout
    }

    pub const fn maximum_response_bytes(self) -> usize {
        self.maximum_response_bytes
    }
}

#[async_trait]
pub trait QueryRequester<Request, Response>: Send + Sync
where
    Request: Send + 'static,
    Response: Send + 'static,
{
    async fn request(
        &self,
        address: &QueryAddress,
        request: QueryRequest<Request>,
        options: QueryOptions,
    ) -> Result<QueryResponse<Response>, QueryRequestError>;
}

#[async_trait]
pub trait QueryHandler<Request, Response>: Send + Sync
where
    Request: Send + 'static,
    Response: Send + 'static,
{
    async fn handle(&self, request: QueryRequest<Request>) -> Result<Response, QueryErrorPayload>;
}

#[async_trait]
pub trait QueryServer<Request, Response>: Send + Sync
where
    Request: Send + 'static,
    Response: Send + 'static,
{
    async fn run(
        &self,
        address: QueryAddress,
        handler: Arc<dyn QueryHandler<Request, Response>>,
    ) -> Result<(), QueryServerError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MessageBuildErrorKind, MAX_ENVELOPE_BYTES};

    const TRACE_PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    fn context() -> EnvelopeContext {
        EnvelopeContext::new(
            MessageId::new("request-1").unwrap(),
            SchemaVersion::new(1).unwrap(),
            CorrelationId::new("correlation-1").unwrap(),
            None,
        )
    }

    #[test]
    fn query_request_preserves_safe_metadata_and_trace_context() {
        let mut metadata = CallerMetadata::new();
        metadata.insert("x-tenant", "acme").unwrap();
        let request = QueryRequest::new(
            context(),
            MessageTimestamp::from_unix_milliseconds(1_700_000_000_000).unwrap(),
            metadata,
            Some(TraceContext::new(TRACE_PARENT).unwrap()),
            serde_json::json!({"order_id": "one"}),
        )
        .unwrap();
        assert_eq!(request.metadata().get("x-tenant"), Some("acme"));
        assert_eq!(
            request.trace_context().unwrap().trace_parent(),
            TRACE_PARENT
        );

        let encoded = serde_json::to_vec(&request).unwrap();
        let decoded: QueryRequest<serde_json::Value> = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn query_errors_preserve_classification_and_application_code() {
        let error = QueryErrorPayload::new(
            QueryErrorClassification::NotFound,
            ApplicationErrorCode::new("orders.not_found").unwrap(),
            "order was not found",
        )
        .unwrap();
        let response = QueryResponse::<serde_json::Value>::error(
            MessageId::new("request-1").unwrap(),
            SchemaVersion::new(1).unwrap(),
            MessageTimestamp::from_unix_milliseconds(1_700_000_000_001).unwrap(),
            CorrelationId::new("correlation-1").unwrap(),
            None,
            error,
        )
        .unwrap();

        let QueryOutcome::Error(error) = response.outcome() else {
            panic!("expected query error");
        };
        assert_eq!(error.classification(), QueryErrorClassification::NotFound);
        assert_eq!(error.code().as_str(), "orders.not_found");

        let encoded = serde_json::to_vec(&response).unwrap();
        let decoded: QueryResponse<serde_json::Value> = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, response);
    }

    #[test]
    fn query_envelopes_and_options_are_bounded() {
        let error = QueryRequest::new(
            context(),
            MessageTimestamp::from_unix_milliseconds(1).unwrap(),
            CallerMetadata::new(),
            None,
            "x".repeat(MAX_ENVELOPE_BYTES),
        )
        .unwrap_err();
        assert_eq!(error.kind(), MessageBuildErrorKind::PayloadTooLarge);
        assert!(QueryOptions::new(Duration::ZERO, 1024).is_err());
        assert!(QueryOptions::new(Duration::from_secs(1), MAX_ENVELOPE_BYTES + 1).is_err());
    }
}
