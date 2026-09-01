use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Request, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response, Sse, sse::Event, sse::KeepAlive},
    routing::{get, post},
};
use futures_util::stream;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::{
    CommandInputError, CorrelationError, DiscoveryError, MAX_COMMAND_PAYLOAD_LEN, OperationMode,
    OperationSnapshot, SimulationRequest, SubmissionError, TestDefinition, TestDefinitionError,
    TestDefinitionValidationError, TestRepositoryError, TestRunError, TestScenarioResetError,
    Tracer, behavioral_test_schema,
};

const IDEMPOTENCY_KEY: &str = "idempotency-key";
const LAST_EVENT_ID: &str = "last-event-id";
const SIMULATION_REQUEST_OVERHEAD: usize = 64 * 1024;

#[derive(Clone)]
pub struct HttpConfig {
    control_token: Arc<str>,
    dispatch_token: Option<Arc<str>>,
}

impl HttpConfig {
    pub fn new(bearer_token: impl Into<String>) -> Result<Self, HttpConfigError> {
        let bearer_token = validate_token(bearer_token.into())?;
        Ok(Self {
            control_token: bearer_token.into(),
            dispatch_token: None,
        })
    }

    pub fn with_dispatch_token(
        mut self,
        dispatch_token: impl Into<String>,
    ) -> Result<Self, HttpConfigError> {
        let dispatch_token = validate_token(dispatch_token.into())?;
        if dispatch_token == self.control_token.as_ref() {
            return Err(HttpConfigError::DuplicateBearerTokens);
        }
        self.dispatch_token = Some(dispatch_token.into());
        Ok(self)
    }
}

fn validate_token(token: String) -> Result<String, HttpConfigError> {
    if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(HttpConfigError::InvalidBearerToken);
    }
    Ok(token)
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum HttpConfigError {
    #[error("tracer bearer token must be non-empty visible ASCII")]
    InvalidBearerToken,
    #[error("tracer and dispatch bearer tokens must differ")]
    DuplicateBearerTokens,
}

#[derive(Clone)]
struct HttpState {
    tracer: Tracer,
    config: HttpConfig,
}

pub fn router(tracer: Tracer, config: HttpConfig) -> Router {
    let control_routes = Router::new()
        .route("/catalog", get(get_catalog))
        .route(
            "/contexts/{context}/aggregates/{aggregate}/instances",
            get(get_aggregate_instances),
        )
        .route(
            "/contexts/{context}/aggregates/{aggregate}/{aggregate_id}/commands/{command}/schemas/{schema_version}/inputs",
            get(get_command_inputs),
        )
        .route(
            "/contexts/{context}/aggregates/{aggregate}/{aggregate_id}/commands/{command}/simulate",
            post(submit_simulation),
        )
        .route(
            "/contexts/{context}/aggregates/{aggregate}/{aggregate_id}/commands/{command}/test",
            post(submit_test),
        )
        .route("/tests", get(get_tests))
        .route("/tests/validate", post(validate_inline_test))
        .route("/tests/{test_id}", get(get_test))
        .route("/tests/{test_id}/runs", post(run_test))
        .route("/test-runs", post(run_inline_test))
        .route("/schemas/behavioral-test-v1", get(get_behavioral_test_schema))
        .route("/test-scenario/reset", post(reset_test_scenario))
        .layer(middleware::from_fn_with_state(
            config.clone(),
            authorize_control_request,
        ));
    let dispatch_routes = Router::new()
        .route(
            "/contexts/{context}/aggregates/{aggregate}/{aggregate_id}/commands/{command}/dispatch",
            post(submit_dispatch),
        )
        .layer(middleware::from_fn_with_state(
            config.clone(),
            authorize_dispatch_request,
        ));
    let operation_routes = Router::new()
        .route("/operations/{operation_id}", get(get_operation))
        .route("/operations/{operation_id}/events", get(operation_events))
        .route(
            "/correlations/{correlation_id}/events",
            get(correlation_events),
        );
    Router::new()
        .merge(control_routes)
        .merge(dispatch_routes)
        .merge(operation_routes)
        .layer(DefaultBodyLimit::max(
            MAX_COMMAND_PAYLOAD_LEN + SIMULATION_REQUEST_OVERHEAD,
        ))
        .layer(middleware::map_response(add_private_no_store))
        .with_state(HttpState { tracer, config })
}

async fn add_private_no_store(response: Response) -> Response {
    private_no_store(response)
}

async fn get_catalog(State(state): State<HttpState>) -> Response {
    let mut catalog = state.tracer.catalog().clone();
    if state.config.dispatch_token.is_none() {
        for version in catalog
            .contexts
            .iter_mut()
            .flat_map(|context| &mut context.aggregates)
            .flat_map(|aggregate| &mut aggregate.commands)
            .flat_map(|command| &mut command.versions)
        {
            version.dispatch_href_template = None;
        }
    }
    no_store(Json(catalog).into_response())
}

async fn get_tests(State(state): State<HttpState>) -> Response {
    match state.tracer.test_definitions() {
        Ok(definitions) => private_no_store(Json(definitions).into_response()),
        Err(error) => test_repository_error_response(&error),
    }
}

async fn get_behavioral_test_schema() -> Response {
    no_store(Json(behavioral_test_schema()).into_response())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDefinitionValidationResponse {
    pub valid: bool,
    pub definition: TestDefinition,
    pub schema_href: &'static str,
    pub run_href: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDefinitionValidationDiagnostic {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

async fn validate_inline_test(
    State(state): State<HttpState>,
    request: Result<Json<Value>, JsonRejection>,
) -> Response {
    let definition = match parse_test_definition(request) {
        Ok(definition) => definition,
        Err(response) => return response,
    };
    if let Err(error) = state.tracer.validate_test_definition(&definition) {
        return runtime_test_definition_error_response(&error);
    }
    private_no_store(
        Json(TestDefinitionValidationResponse {
            valid: true,
            definition,
            schema_href: "/schemas/behavioral-test-v1",
            run_href: "/test-runs",
        })
        .into_response(),
    )
}

async fn run_inline_test(
    State(state): State<HttpState>,
    request: Result<Json<Value>, JsonRejection>,
) -> Response {
    let definition = match parse_test_definition(request) {
        Ok(definition) => definition,
        Err(response) => return response,
    };
    match state.tracer.run_inline_test(definition).await {
        Ok(report) => private_no_store(Json(report).into_response()),
        Err(error) => test_run_error_response(&error),
    }
}

#[allow(clippy::result_large_err)]
fn parse_test_definition(
    request: Result<Json<Value>, JsonRejection>,
) -> Result<TestDefinition, Response> {
    let Json(value) = request.map_err(|rejection| json_rejection(&rejection))?;
    TestDefinition::from_json_value(value).map_err(|error| test_definition_error_response(&error))
}

async fn get_test(State(state): State<HttpState>, Path(test_id): Path<String>) -> Response {
    match state.tracer.test_definition(&test_id) {
        Ok(definition) => private_no_store(Json(definition).into_response()),
        Err(error) => test_repository_error_response(&error),
    }
}

async fn run_test(State(state): State<HttpState>, Path(test_id): Path<String>) -> Response {
    match state.tracer.run_test(&test_id).await {
        Ok(report) => private_no_store(Json(report).into_response()),
        Err(error) => test_run_error_response(&error),
    }
}

async fn get_aggregate_instances(
    State(state): State<HttpState>,
    Path((context, aggregate)): Path<(String, String)>,
) -> Response {
    let aggregate_type = format!("{context}/{aggregate}");
    match state.tracer.aggregate_instances(&aggregate_type).await {
        Ok(instances) => no_store(Json(instances).into_response()),
        Err(error) => discovery_error_response(&error),
    }
}

async fn get_command_inputs(
    State(state): State<HttpState>,
    Path((context, aggregate, aggregate_id, command, schema_version)): Path<(
        String,
        String,
        String,
        String,
        u32,
    )>,
) -> Response {
    let aggregate_type = format!("{context}/{aggregate}");
    match state
        .tracer
        .command_inputs(&aggregate_type, &aggregate_id, &command, schema_version)
        .await
    {
        Ok(inputs) => no_store(Json(inputs).into_response()),
        Err(error) => command_input_error_response(&error),
    }
}

async fn authorize_control_request(
    State(config): State<HttpConfig>,
    request: Request,
    next: Next,
) -> Response {
    if capability(&config, request.headers()) != Some(Capability::Control) {
        return unauthorized();
    }
    next.run(request).await
}

async fn authorize_dispatch_request(
    State(config): State<HttpConfig>,
    request: Request,
    next: Next,
) -> Response {
    match capability(&config, request.headers()) {
        Some(Capability::Dispatch) => next.run(request).await,
        Some(Capability::Control) => forbidden(),
        None => unauthorized(),
    }
}

async fn submit_simulation(
    State(state): State<HttpState>,
    Path((context, aggregate, aggregate_id, command)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    request: Result<Json<SimulationRequest>, JsonRejection>,
) -> Response {
    submit_command(
        &state.tracer,
        OperationMode::Simulate,
        (context, aggregate, aggregate_id, command),
        &headers,
        request,
    )
    .await
}

async fn submit_test(
    State(state): State<HttpState>,
    Path(path): Path<(String, String, String, String)>,
    headers: HeaderMap,
    request: Result<Json<SimulationRequest>, JsonRejection>,
) -> Response {
    submit_command(&state.tracer, OperationMode::Test, path, &headers, request).await
}

async fn submit_dispatch(
    State(state): State<HttpState>,
    Path(path): Path<(String, String, String, String)>,
    headers: HeaderMap,
    request: Result<Json<SimulationRequest>, JsonRejection>,
) -> Response {
    submit_command(
        &state.tracer,
        OperationMode::Dispatch,
        path,
        &headers,
        request,
    )
    .await
}

async fn submit_command(
    tracer: &Tracer,
    mode: OperationMode,
    (context, aggregate, aggregate_id, command): (String, String, String, String),
    headers: &HeaderMap,
    request: Result<Json<SimulationRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match request {
        Ok(request) => request,
        Err(rejection) => return json_rejection(&rejection),
    };
    let idempotency_key = match optional_header(headers, IDEMPOTENCY_KEY) {
        Ok(value) => value,
        Err(message) => return bad_request(&message),
    };
    let aggregate_type = format!("{context}/{aggregate}");
    let result = match mode {
        OperationMode::Simulate => {
            tracer
                .submit_simulation(
                    &aggregate_type,
                    &aggregate_id,
                    &command,
                    request,
                    idempotency_key,
                )
                .await
        }
        OperationMode::Test => {
            tracer
                .submit_test(
                    &aggregate_type,
                    &aggregate_id,
                    &command,
                    request,
                    idempotency_key,
                )
                .await
        }
        OperationMode::Dispatch => {
            tracer
                .submit_dispatch(
                    &aggregate_type,
                    &aggregate_id,
                    &command,
                    request,
                    idempotency_key,
                )
                .await
        }
    };
    match result {
        Ok(operation) => {
            let location = format!("/operations/{}", operation.operation_id);
            (
                StatusCode::ACCEPTED,
                [(header::LOCATION, location)],
                Json(operation),
            )
                .into_response()
        }
        Err(error) => error_response(&error),
    }
}

async fn reset_test_scenario(State(state): State<HttpState>) -> Response {
    match state.tracer.reset_test_scenario().await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => test_scenario_error_response(&error),
    }
}

async fn get_operation(
    State(state): State<HttpState>,
    Path(operation_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    match authorized_operation(&state, &headers, &operation_id).await {
        Ok(operation) => private_no_store(Json(operation).into_response()),
        Err(response) => response,
    }
}

async fn operation_events(
    State(state): State<HttpState>,
    Path(operation_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(capability) = capability(&state.config, &headers) else {
        return unauthorized();
    };
    let after = match optional_header(&headers, LAST_EVENT_ID) {
        Ok(Some(value)) => match value.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return bad_request("Last-Event-ID must be an unsigned integer"),
        },
        Ok(None) => 0,
        Err(message) => return bad_request(&message),
    };
    let operation = match state.tracer.operation(&operation_id).await {
        Ok(operation) => operation,
        Err(error) => return error_response(&error),
    };
    if let Err(response) = authorize_mode(capability, operation.mode) {
        return response;
    }
    let subscription = match state.tracer.subscribe(&operation_id, after).await {
        Ok(subscription) => subscription,
        Err(error) => return error_response(&error),
    };
    if subscription.is_complete().await {
        return StatusCode::NO_CONTENT.into_response();
    }
    let stream = stream::unfold(subscription, |mut subscription| async move {
        let event = subscription.next().await?;
        let frame = serde_json::to_string(&event).map(|data| {
            Event::default()
                .id(event.id.to_string())
                .event(event.kind.event_name())
                .data(data)
        });
        Some((frame, subscription))
    });
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

async fn correlation_events(
    State(state): State<HttpState>,
    Path(correlation_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(capability) = capability(&state.config, &headers) else {
        return unauthorized();
    };
    let after = match optional_header(&headers, LAST_EVENT_ID) {
        Ok(Some(value)) => match value.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return bad_request("Last-Event-ID must be an unsigned integer"),
        },
        Ok(None) => 0,
        Err(message) => return bad_request(&message),
    };
    let mode = match state.tracer.correlation_mode(&correlation_id) {
        Ok(mode) => mode,
        Err(error) => return correlation_error_response(&error),
    };
    if let Err(response) = authorize_mode(capability, mode) {
        return response;
    }
    let subscription = match state
        .tracer
        .subscribe_correlation(&correlation_id, after)
        .await
    {
        Ok(subscription) => subscription,
        Err(error) => return correlation_error_response(&error),
    };
    let stream = stream::unfold(subscription, |mut subscription| async move {
        let event = subscription.next().await?;
        let frame = serde_json::to_string(&event).map(|data| {
            Event::default()
                .id(event.id.to_string())
                .event(event.kind.event_name())
                .data(data)
        });
        Some((frame, subscription))
    });
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

fn optional_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
) -> Result<Option<&'a str>, String> {
    headers
        .get(name)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| format!("{name} must contain visible ASCII"))
        })
        .transpose()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Capability {
    Control,
    Dispatch,
}

fn capability(config: &HttpConfig, headers: &HeaderMap) -> Option<Capability> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))?;
    if token == config.control_token.as_ref() {
        Some(Capability::Control)
    } else if config
        .dispatch_token
        .as_ref()
        .is_some_and(|expected| token == expected.as_ref())
    {
        Some(Capability::Dispatch)
    } else {
        None
    }
}

#[allow(clippy::result_large_err)]
async fn authorized_operation(
    state: &HttpState,
    headers: &HeaderMap,
    operation_id: &str,
) -> Result<OperationSnapshot, Response> {
    let capability = capability(&state.config, headers).ok_or_else(unauthorized)?;
    let operation = state
        .tracer
        .operation(operation_id)
        .await
        .map_err(|error| error_response(&error))?;
    authorize_mode(capability, operation.mode)?;
    Ok(operation)
}

#[allow(clippy::result_large_err)]
fn authorize_mode(capability: Capability, mode: OperationMode) -> Result<(), Response> {
    let required = if mode == OperationMode::Dispatch {
        Capability::Dispatch
    } else {
        Capability::Control
    };
    if capability != required {
        return Err(forbidden());
    }
    Ok(())
}

fn private_no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

fn unauthorized() -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(ErrorBody {
            code: "unauthorized",
            message: "a valid bearer capability is required".to_owned(),
        }),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorBody {
            code: "forbidden",
            message: "the bearer capability does not permit this operation".to_owned(),
        }),
    )
        .into_response()
}

fn json_rejection(rejection: &JsonRejection) -> Response {
    let code = match rejection.status() {
        StatusCode::PAYLOAD_TOO_LARGE => "payload-too-large",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported-media-type",
        _ => "invalid-json",
    };
    (
        rejection.status(),
        Json(ErrorBody {
            code,
            message: rejection.body_text(),
        }),
    )
        .into_response()
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TestDefinitionErrorBody {
    code: &'static str,
    message: String,
    issues: Vec<TestDefinitionValidationDiagnostic>,
}

fn test_definition_error_response(error: &TestDefinitionError) -> Response {
    let issues = error
        .issues()
        .iter()
        .map(|issue| TestDefinitionValidationDiagnostic {
            code: issue.code(),
            path: issue.path().to_owned(),
            message: issue.message().to_owned(),
        })
        .collect();
    invalid_test_definition_response(error.to_string(), issues)
}

fn runtime_test_definition_error_response(error: &TestDefinitionValidationError) -> Response {
    let (code, path) = match error {
        TestDefinitionValidationError::MissingSubject { .. } => {
            ("missing-subject", "/expected/graphs/0/nodes")
        }
        TestDefinitionValidationError::FixtureUnavailable { .. } => {
            ("fixture-unavailable", "/setup/fixture")
        }
        TestDefinitionValidationError::FixtureMismatch { .. } => {
            ("fixture-mismatch", "/setup/fixture")
        }
        TestDefinitionValidationError::UnknownCommand { path, .. } => {
            ("unknown-command", path.as_str())
        }
        TestDefinitionValidationError::InvalidCommandPayload { path, .. } => {
            ("invalid-command-payload", path.as_str())
        }
        TestDefinitionValidationError::InvalidAggregateId { path, .. } => {
            ("invalid-aggregate-id", path.as_str())
        }
        TestDefinitionValidationError::CommandPayloadTooLarge { path, .. } => {
            ("command-payload-too-large", path.as_str())
        }
    };
    invalid_test_definition_response(
        error.to_string(),
        vec![TestDefinitionValidationDiagnostic {
            code,
            path: path.to_owned(),
            message: error.to_string(),
        }],
    )
}

fn invalid_test_definition_response(
    message: String,
    issues: Vec<TestDefinitionValidationDiagnostic>,
) -> Response {
    private_no_store(
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(TestDefinitionErrorBody {
                code: "invalid-test-definition",
                message,
                issues,
            }),
        )
            .into_response(),
    )
}

fn error_response(error: &SubmissionError) -> Response {
    let (status, code, retry_after) = match error {
        SubmissionError::UnknownCommand { .. } | SubmissionError::NotFound => {
            (StatusCode::NOT_FOUND, "not-found", false)
        }
        SubmissionError::IdentityConflict => (StatusCode::CONFLICT, "identity-conflict", false),
        SubmissionError::IdempotencyKeyRequired => {
            (StatusCode::BAD_REQUEST, "idempotency-key-required", false)
        }
        SubmissionError::TestScenarioUnavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "test-scenario-unavailable",
            true,
        ),
        SubmissionError::CapacityExhausted => {
            (StatusCode::SERVICE_UNAVAILABLE, "capacity-exhausted", true)
        }
        SubmissionError::ConcurrencyExhausted => (
            StatusCode::SERVICE_UNAVAILABLE,
            "concurrency-exhausted",
            true,
        ),
        SubmissionError::ModeUnavailable(_) => {
            (StatusCode::NOT_IMPLEMENTED, "mode-unavailable", false)
        }
        SubmissionError::InvalidAggregateId(_) | SubmissionError::InvalidOperationId(_) => {
            (StatusCode::BAD_REQUEST, "invalid-request", false)
        }
        SubmissionError::InvalidCursor(_) => (StatusCode::BAD_REQUEST, "future-cursor", false),
        SubmissionError::PayloadTooLarge { .. } => {
            (StatusCode::PAYLOAD_TOO_LARGE, "payload-too-large", false)
        }
    };
    let mut response = (
        status,
        Json(ErrorBody {
            code,
            message: error.to_string(),
        }),
    )
        .into_response();
    if retry_after {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}

fn correlation_error_response(error: &CorrelationError) -> Response {
    let (status, code) = match error {
        CorrelationError::InvalidId(_) => (StatusCode::BAD_REQUEST, "invalid-correlation"),
        CorrelationError::NotFound => (StatusCode::NOT_FOUND, "not-found"),
        CorrelationError::CapacityExhausted => {
            (StatusCode::SERVICE_UNAVAILABLE, "capacity-exhausted")
        }
        CorrelationError::EventTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "payload-too-large"),
        CorrelationError::FutureCursor { .. } => (StatusCode::BAD_REQUEST, "future-cursor"),
        CorrelationError::ExpiredCursor { .. } => (StatusCode::GONE, "expired-cursor"),
    };
    (
        status,
        Json(ErrorBody {
            code,
            message: error.to_string(),
        }),
    )
        .into_response()
}

fn test_scenario_error_response(error: &TestScenarioResetError) -> Response {
    let (status, code, retry_after) = match error {
        TestScenarioResetError::Unavailable => {
            (StatusCode::NOT_IMPLEMENTED, "reset-unavailable", false)
        }
        TestScenarioResetError::Failed(_) => {
            (StatusCode::SERVICE_UNAVAILABLE, "reset-failed", true)
        }
    };
    let mut response = (
        status,
        Json(ErrorBody {
            code,
            message: error.to_string(),
        }),
    )
        .into_response();
    if retry_after {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}

fn test_repository_error_response(error: &TestRepositoryError) -> Response {
    let (status, code) = match error {
        TestRepositoryError::Unavailable => (StatusCode::NOT_IMPLEMENTED, "tests-unavailable"),
        TestRepositoryError::NotFound(_) => (StatusCode::NOT_FOUND, "not-found"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "test-repository-invalid"),
    };
    private_no_store(
        (
            status,
            Json(ErrorBody {
                code,
                message: error.to_string(),
            }),
        )
            .into_response(),
    )
}

fn test_run_error_response(error: &TestRunError) -> Response {
    let (status, code, retry_after) = match error {
        TestRunError::Repository(error) => return test_repository_error_response(error),
        TestRunError::Reset(error) => return test_scenario_error_response(error),
        TestRunError::Validation(error) => {
            return runtime_test_definition_error_response(error);
        }
        TestRunError::Submission(error) => return private_no_store(error_response(error)),
        TestRunError::Correlation(error) => {
            return private_no_store(correlation_error_response(error));
        }
        TestRunError::FixtureMismatch { .. } | TestRunError::SetupRejected { .. } => {
            (StatusCode::CONFLICT, "test-setup-failed", false)
        }
        TestRunError::CommandFailed(_) | TestRunError::CorrelationClosed => {
            (StatusCode::SERVICE_UNAVAILABLE, "test-run-failed", true)
        }
    };
    let mut response = (
        status,
        Json(ErrorBody {
            code,
            message: error.to_string(),
        }),
    )
        .into_response();
    if retry_after {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    private_no_store(response)
}

fn discovery_error_response(error: &DiscoveryError) -> Response {
    let (status, code, retry_after) = match error {
        DiscoveryError::UnknownAggregate { .. } => (StatusCode::NOT_FOUND, "not-found", false),
        DiscoveryError::InstanceDiscoveryUnavailable => {
            (StatusCode::NOT_IMPLEMENTED, "not-supported", false)
        }
        DiscoveryError::TestScenarioUnavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "test-scenario-unavailable",
            true,
        ),
        DiscoveryError::Directory(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "directory-unavailable",
            true,
        ),
    };
    let mut response = (
        status,
        Json(ErrorBody {
            code,
            message: error.to_string(),
        }),
    )
        .into_response();
    if retry_after {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}

fn command_input_error_response(error: &CommandInputError) -> Response {
    let (status, code, retry_after) = match error {
        CommandInputError::UnknownCommand { .. } => (StatusCode::NOT_FOUND, "not-found", false),
        CommandInputError::InvalidAggregateId(_) => {
            (StatusCode::BAD_REQUEST, "invalid-request", false)
        }
        CommandInputError::TestScenarioUnavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "test-scenario-unavailable",
            true,
        ),
        CommandInputError::Runtime(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "input-discovery-failed",
            true,
        ),
    };
    let mut response = (
        status,
        Json(ErrorBody {
            code,
            message: error.to_string(),
        }),
    )
        .into_response();
    if retry_after {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorBody {
            code: "invalid-request",
            message: message.to_owned(),
        }),
    )
        .into_response()
}
