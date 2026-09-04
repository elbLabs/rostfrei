use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::post,
};
use rostfrei::{
    CommandBus, CommandBusError, CommandBusErrorKind, CommandRejection,
    CommandRejectionClassification, CommandResponseOutcome, DomainRegistry, DynamicCommandRequest,
    DynamicQueryRequest, OperationId, QueryBus, QueryBusError, QueryBusErrorKind,
    QueryErrorClassification, QueryErrorPayload, QueryOptions, QueryOutcome, StreamAggregateId,
    TraceContext,
};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

pub const DEFAULT_MAXIMUM_COMMAND_PAYLOAD_BYTES: usize = 1024 * 1024;
pub const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";
pub const TRACE_PARENT_HEADER: &str = "traceparent";
pub const TRACE_STATE_HEADER: &str = "tracestate";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpApiConfig {
    query_options: QueryOptions,
    maximum_command_payload_bytes: usize,
    authentication_challenge: Option<HeaderValue>,
}

impl HttpApiConfig {
    pub const fn new(
        query_options: QueryOptions,
        maximum_command_payload_bytes: usize,
    ) -> Result<Self, HttpApiConfigError> {
        if maximum_command_payload_bytes == 0 {
            return Err(HttpApiConfigError::ZeroCommandPayloadLimit);
        }
        Ok(Self {
            query_options,
            maximum_command_payload_bytes,
            authentication_challenge: None,
        })
    }

    pub fn with_authentication_challenge(
        mut self,
        challenge: HeaderValue,
    ) -> Result<Self, HttpApiConfigError> {
        if !valid_authentication_challenge(&challenge) {
            return Err(HttpApiConfigError::InvalidAuthenticationChallenge);
        }
        self.authentication_challenge = Some(challenge);
        Ok(self)
    }

    pub const fn query_options(&self) -> QueryOptions {
        self.query_options
    }

    pub const fn maximum_command_payload_bytes(&self) -> usize {
        self.maximum_command_payload_bytes
    }

    pub const fn authentication_challenge(&self) -> Option<&HeaderValue> {
        self.authentication_challenge.as_ref()
    }
}

impl Default for HttpApiConfig {
    fn default() -> Self {
        Self {
            query_options: QueryOptions::default(),
            maximum_command_payload_bytes: DEFAULT_MAXIMUM_COMMAND_PAYLOAD_BYTES,
            authentication_challenge: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum HttpApiConfigError {
    #[error("maximum command payload bytes must be greater than zero")]
    ZeroCommandPayloadLimit,
    #[error("authentication challenge must contain a valid visible-ASCII authentication scheme")]
    InvalidAuthenticationChallenge,
}

fn valid_authentication_challenge(challenge: &HeaderValue) -> bool {
    let Ok(challenge) = challenge.to_str() else {
        return false;
    };
    if challenge.is_empty()
        || challenge.trim() != challenge
        || !challenge.bytes().all(|byte| matches!(byte, b' '..=b'~'))
    {
        return false;
    }
    let scheme = challenge
        .split_once(' ')
        .map_or(challenge, |(scheme, _)| scheme);
    !scheme.is_empty()
        && scheme.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

#[derive(Clone)]
struct HttpState {
    registry: Arc<DomainRegistry>,
    command_bus: CommandBus,
    query_bus: QueryBus,
    query_options: QueryOptions,
    authentication_challenge: Option<HeaderValue>,
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "router construction takes ownership of its complete configuration"
)]
pub fn router(
    registry: Arc<DomainRegistry>,
    command_bus: CommandBus,
    query_bus: QueryBus,
    config: HttpApiConfig,
) -> Router {
    let maximum_command_payload_bytes = config.maximum_command_payload_bytes();
    Router::new()
        .route(
            "/contexts/{context}/queries/{query}/schemas/{schema_version}",
            post(submit_query),
        )
        .route(
            "/contexts/{context}/aggregates/{aggregate}/{aggregate_id}/commands/{command}/schemas/{schema_version}",
            post(submit_command),
        )
        .layer(DefaultBodyLimit::max(maximum_command_payload_bytes))
        .layer(middleware::map_response(add_private_no_store))
        .with_state(HttpState {
            registry,
            command_bus,
            query_bus,
            query_options: config.query_options(),
            authentication_challenge: config.authentication_challenge().cloned(),
        })
}

async fn submit_query(
    State(state): State<HttpState>,
    Path((context, query, schema_version)): Path<(String, String, u32)>,
    headers: HeaderMap,
    payload: Result<Json<Value>, JsonRejection>,
) -> Response {
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            let code = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                "rostfrei.http.payload-too-large"
            } else {
                "rostfrei.http.invalid-json"
            };
            return error_response(rejection.status(), code, rejection.body_text());
        }
    };
    if context != state.query_bus.context().name().as_str()
        || state
            .registry
            .query(&context, &query, schema_version)
            .is_none()
    {
        return not_found("The query name or schema version is not registered.");
    }
    let trace_context = match trace_context(&headers) {
        Ok(trace_context) => trace_context,
        Err(error) => return error.into_response(),
    };
    let request = match DynamicQueryRequest::new(query, schema_version, payload) {
        Ok(request) => match trace_context {
            Some(trace_context) => request.with_trace_context(trace_context),
            None => request,
        },
        Err(error) => return query_bus_error(&error),
    };
    match state
        .query_bus
        .request_dynamic(request, state.query_options)
        .await
    {
        Ok(response) => match response.into_outcome() {
            QueryOutcome::Success(payload) => Json(payload).into_response(),
            QueryOutcome::Error(error) => {
                query_application_error(error, state.authentication_challenge.as_ref())
            }
        },
        Err(error) => query_bus_error(&error),
    }
}

async fn submit_command(
    State(state): State<HttpState>,
    Path((context, aggregate, aggregate_id, command, schema_version)): Path<(
        String,
        String,
        String,
        String,
        u32,
    )>,
    headers: HeaderMap,
    payload: Result<Json<Value>, JsonRejection>,
) -> Response {
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            let code = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                "rostfrei.http.payload-too-large"
            } else {
                "rostfrei.http.invalid-json"
            };
            return error_response(rejection.status(), code, rejection.body_text());
        }
    };
    if context != state.command_bus.context().name().as_str() {
        return not_found("The command name or schema version is not registered.");
    }
    if aggregate.contains('/') {
        return bad_request(
            "rostfrei.http.invalid-aggregate",
            "aggregate path segment must not contain a context separator",
        );
    }
    let qualified_aggregate_type = format!("{context}/{aggregate}");
    let aggregate_type = match state
        .registry
        .command(&qualified_aggregate_type, &command, schema_version)
        .or_else(|| state.registry.command(&aggregate, &command, schema_version))
    {
        Some(descriptor) => descriptor.aggregate_type.clone(),
        None => return not_found("The command name or schema version is not registered."),
    };
    let operation_id = match idempotency_key(&headers) {
        Ok(operation_id) => operation_id,
        Err(error) => return error.into_response(),
    };
    let aggregate_id = match StreamAggregateId::new(aggregate_id) {
        Ok(aggregate_id) => aggregate_id,
        Err(error) => {
            return bad_request("rostfrei.http.invalid-aggregate-id", error.to_string());
        }
    };
    let request = match DynamicCommandRequest::new(
        operation_id,
        aggregate_type,
        aggregate_id,
        command,
        schema_version,
        payload,
    ) {
        Ok(request) => request,
        Err(error) => return command_bus_error(&error),
    };
    match state.command_bus.dispatch_dynamic(request).await {
        Ok(receipt) => command_response(
            receipt.into_response(),
            state.authentication_challenge.as_ref(),
        ),
        Err(error) => command_bus_error(&error),
    }
}

fn idempotency_key(headers: &HeaderMap) -> Result<OperationId, HttpRequestError> {
    let value = required_single_header(headers, IDEMPOTENCY_KEY_HEADER)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(HttpRequestError::new(
            "rostfrei.http.invalid-idempotency-key",
            "idempotency-key must contain only ASCII letters, digits, '-', '_', '.', or ':'",
        ));
    }
    OperationId::new(value).map_err(|error| {
        HttpRequestError::new("rostfrei.http.invalid-idempotency-key", error.to_string())
    })
}

fn trace_context(headers: &HeaderMap) -> Result<Option<TraceContext>, HttpRequestError> {
    let trace_parent = optional_single_header(headers, TRACE_PARENT_HEADER)?;
    let trace_state = optional_single_header(headers, TRACE_STATE_HEADER)?;
    match trace_parent {
        Some(trace_parent) => TraceContext::from_parts(trace_parent, trace_state)
            .map(Some)
            .map_err(|error| {
                HttpRequestError::new("rostfrei.http.invalid-trace-context", error.to_string())
            }),
        None if trace_state.is_some() => Err(HttpRequestError::new(
            "rostfrei.http.invalid-trace-context",
            "tracestate requires traceparent",
        )),
        None => Ok(None),
    }
}

fn required_single_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
) -> Result<&'a str, HttpRequestError> {
    optional_single_header(headers, name)?.ok_or_else(|| {
        HttpRequestError::new(
            "rostfrei.http.missing-header",
            format!("{name} is required"),
        )
    })
}

fn optional_single_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
) -> Result<Option<&'a str>, HttpRequestError> {
    let mut values = headers.get_all(name).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(HttpRequestError::new(
            "rostfrei.http.duplicate-header",
            format!("{name} must occur exactly once"),
        ));
    }
    value.to_str().map(Some).map_err(|_| {
        HttpRequestError::new(
            "rostfrei.http.invalid-header",
            format!("{name} must contain visible ASCII"),
        )
    })
}

fn command_response(
    response: rostfrei::CommandResponse,
    authentication_challenge: Option<&HeaderValue>,
) -> Response {
    let operation_id = response.operation_id().as_str().to_owned();
    let correlation_id = response.correlation_id().as_str().to_owned();
    match response.into_outcome() {
        CommandResponseOutcome::Accepted => Json(CommandHttpResponse::Accepted {
            operation_id,
            correlation_id,
        })
        .into_response(),
        CommandResponseOutcome::Rejected(error) => {
            let status = command_rejection_status(
                error.classification(),
                authentication_challenge.is_some(),
            );
            let mut response = (
                status,
                Json(CommandHttpResponse::Rejected {
                    operation_id,
                    correlation_id,
                    error,
                }),
            )
                .into_response();
            add_authentication_challenge(&mut response, status, authentication_challenge);
            response
        }
    }
}

fn query_application_error(
    error: QueryErrorPayload,
    authentication_challenge: Option<&HeaderValue>,
) -> Response {
    let status = query_error_status(error.classification(), authentication_challenge.is_some());
    let mut response = (status, Json(QueryErrorBody { error })).into_response();
    add_authentication_challenge(&mut response, status, authentication_challenge);
    response
}

const fn command_rejection_status(
    classification: CommandRejectionClassification,
    has_authentication_challenge: bool,
) -> StatusCode {
    match classification {
        CommandRejectionClassification::InvalidRequest => StatusCode::BAD_REQUEST,
        CommandRejectionClassification::Unauthorized if has_authentication_challenge => {
            StatusCode::UNAUTHORIZED
        }
        CommandRejectionClassification::Unauthorized
        | CommandRejectionClassification::Forbidden => StatusCode::FORBIDDEN,
        CommandRejectionClassification::NotFound => StatusCode::NOT_FOUND,
        CommandRejectionClassification::Conflict => StatusCode::CONFLICT,
        CommandRejectionClassification::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        CommandRejectionClassification::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

const fn query_error_status(
    classification: QueryErrorClassification,
    has_authentication_challenge: bool,
) -> StatusCode {
    match classification {
        QueryErrorClassification::InvalidRequest => StatusCode::BAD_REQUEST,
        QueryErrorClassification::Unauthorized if has_authentication_challenge => {
            StatusCode::UNAUTHORIZED
        }
        QueryErrorClassification::Unauthorized | QueryErrorClassification::Forbidden => {
            StatusCode::FORBIDDEN
        }
        QueryErrorClassification::NotFound => StatusCode::NOT_FOUND,
        QueryErrorClassification::Conflict => StatusCode::CONFLICT,
        QueryErrorClassification::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        QueryErrorClassification::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        QueryErrorClassification::Timeout => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn add_authentication_challenge(
    response: &mut Response,
    status: StatusCode,
    authentication_challenge: Option<&HeaderValue>,
) {
    if status == StatusCode::UNAUTHORIZED
        && let Some(authentication_challenge) = authentication_challenge
    {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, authentication_challenge.clone());
    }
}

fn command_bus_error(error: &CommandBusError) -> Response {
    let (status, code, message) = match error.kind() {
        CommandBusErrorKind::Encoding => (
            StatusCode::BAD_REQUEST,
            "rostfrei.http.invalid-command",
            "The command request is invalid.",
        ),
        CommandBusErrorKind::PayloadTooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "rostfrei.http.payload-too-large",
            "The command payload is too large.",
        ),
        CommandBusErrorKind::Timeout => (
            StatusCode::GATEWAY_TIMEOUT,
            "rostfrei.http.command-timeout",
            "The command response timed out.",
        ),
        CommandBusErrorKind::Rejected | CommandBusErrorKind::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "rostfrei.http.command-unavailable",
            "Command messaging is unavailable.",
        ),
        CommandBusErrorKind::InvalidMessage | CommandBusErrorKind::InvalidResponse => (
            StatusCode::BAD_GATEWAY,
            "rostfrei.http.invalid-command-response",
            "Command messaging returned an invalid response.",
        ),
        CommandBusErrorKind::InvalidConfiguration => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "rostfrei.http.command-configuration",
            "Command messaging is not configured correctly.",
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "rostfrei.http.command-failed",
            "The command could not be completed.",
        ),
    };
    error_response(status, code, message)
}

fn query_bus_error(error: &QueryBusError) -> Response {
    let (status, code, message) = match error.kind() {
        QueryBusErrorKind::Encoding => (
            StatusCode::BAD_REQUEST,
            "rostfrei.http.invalid-query",
            "The query request is invalid.",
        ),
        QueryBusErrorKind::Timeout => (
            StatusCode::GATEWAY_TIMEOUT,
            "rostfrei.http.query-timeout",
            "The query response timed out.",
        ),
        QueryBusErrorKind::Rejected | QueryBusErrorKind::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "rostfrei.http.query-unavailable",
            "Query messaging is unavailable.",
        ),
        QueryBusErrorKind::InvalidResponse => (
            StatusCode::BAD_GATEWAY,
            "rostfrei.http.invalid-query-response",
            "Query messaging returned an invalid response.",
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "rostfrei.http.query-failed",
            "The query could not be completed.",
        ),
    };
    error_response(status, code, message)
}

fn not_found(message: impl Into<String>) -> Response {
    error_response(StatusCode::NOT_FOUND, "rostfrei.http.not-found", message)
}

fn bad_request(code: &'static str, message: impl Into<String>) -> Response {
    error_response(StatusCode::BAD_REQUEST, code, message)
}

fn error_response(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    (
        status,
        Json(ErrorBody {
            code,
            message: message.into(),
        }),
    )
        .into_response()
}

async fn add_private_no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum CommandHttpResponse {
    Accepted {
        operation_id: String,
        correlation_id: String,
    },
    Rejected {
        operation_id: String,
        correlation_id: String,
        error: CommandRejection,
    },
}

#[derive(Serialize)]
struct QueryErrorBody {
    error: QueryErrorPayload,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

struct HttpRequestError {
    code: &'static str,
    message: String,
}

impl HttpRequestError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn into_response(self) -> Response {
        bad_request(self.code, self.message)
    }
}
