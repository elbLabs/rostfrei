use std::{convert::Infallible, sync::Arc, time::Duration};

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
use thiserror::Error;

use crate::{ControlPlane, MAX_COMMAND_PAYLOAD_LEN, SimulationRequest, SubmissionError};

const IDEMPOTENCY_KEY: &str = "idempotency-key";
const LAST_EVENT_ID: &str = "last-event-id";
const SIMULATION_REQUEST_OVERHEAD: usize = 64 * 1024;

#[derive(Clone)]
pub struct HttpConfig {
    bearer_token: Arc<str>,
}

impl HttpConfig {
    pub fn new(bearer_token: impl Into<String>) -> Result<Self, HttpConfigError> {
        let bearer_token = bearer_token.into();
        if bearer_token.is_empty() || !bearer_token.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(HttpConfigError::InvalidBearerToken);
        }
        Ok(Self {
            bearer_token: bearer_token.into(),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum HttpConfigError {
    #[error("control-plane bearer token must be non-empty visible ASCII")]
    InvalidBearerToken,
}

#[derive(Clone)]
struct HttpState {
    control_plane: ControlPlane,
}

pub fn router(control_plane: ControlPlane, config: HttpConfig) -> Router {
    Router::new()
        .route(
            "/v1/contexts/{context}/aggregates/{aggregate}/{aggregate_id}/commands/{command}/simulate",
            post(submit_simulation),
        )
        .route("/v1/operations/{operation_id}", get(get_operation))
        .route(
            "/v1/operations/{operation_id}/events",
            get(operation_events),
        )
        .layer(DefaultBodyLimit::max(
            MAX_COMMAND_PAYLOAD_LEN + SIMULATION_REQUEST_OVERHEAD,
        ))
        .layer(middleware::from_fn_with_state(config, authorize_request))
        .with_state(HttpState { control_plane })
}

async fn authorize_request(
    State(config): State<HttpConfig>,
    request: Request,
    next: Next,
) -> Response {
    if !is_authorized(&config, request.headers()) {
        return unauthorized();
    }
    next.run(request).await
}

async fn submit_simulation(
    State(state): State<HttpState>,
    Path((context, aggregate, aggregate_id, command)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    request: Result<Json<SimulationRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match request {
        Ok(request) => request,
        Err(rejection) => return json_rejection(&rejection),
    };
    let idempotency_key = match optional_header(&headers, IDEMPOTENCY_KEY) {
        Ok(value) => value,
        Err(message) => return bad_request(&message),
    };
    let aggregate_type = format!("{context}/{aggregate}");
    match state
        .control_plane
        .submit_simulation(
            &aggregate_type,
            &aggregate_id,
            &command,
            request,
            idempotency_key,
        )
        .await
    {
        Ok(operation) => {
            let location = format!("/v1/operations/{}", operation.operation_id);
            let mut response = (StatusCode::ACCEPTED, Json(operation)).into_response();
            response.headers_mut().insert(
                header::LOCATION,
                HeaderValue::from_str(&location).expect("validated operation ID creates a header"),
            );
            response
        }
        Err(error) => error_response(&error),
    }
}

async fn get_operation(
    State(state): State<HttpState>,
    Path(operation_id): Path<String>,
) -> Response {
    match state.control_plane.operation(&operation_id).await {
        Ok(operation) => Json(operation).into_response(),
        Err(error) => error_response(&error),
    }
}

async fn operation_events(
    State(state): State<HttpState>,
    Path(operation_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let after = match optional_header(&headers, LAST_EVENT_ID) {
        Ok(Some(value)) => match value.parse::<u64>() {
            Ok(value) => value,
            Err(_) => return bad_request("Last-Event-ID must be an unsigned integer"),
        },
        Ok(None) => 0,
        Err(message) => return bad_request(&message),
    };
    let subscription = match state.control_plane.subscribe(&operation_id, after).await {
        Ok(subscription) => subscription,
        Err(error) => return error_response(&error),
    };
    if subscription.is_complete().await {
        return StatusCode::NO_CONTENT.into_response();
    }
    let stream = stream::unfold(subscription, |mut subscription| async move {
        let event = subscription.next().await?;
        let data =
            serde_json::to_string(&event).expect("operation events always serialize successfully");
        let frame = Event::default()
            .id(event.id.to_string())
            .event(event.kind.event_name())
            .data(data);
        Some((Ok::<_, Infallible>(frame), subscription))
    });
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
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

fn is_authorized(config: &HttpConfig, headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == config.bearer_token.as_ref())
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

fn json_rejection(rejection: &JsonRejection) -> Response {
    let code = if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        "payload-too-large"
    } else {
        "invalid-json"
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

fn error_response(error: &SubmissionError) -> Response {
    let (status, code, retry_after) = match error {
        SubmissionError::UnknownCommand { .. } | SubmissionError::NotFound => {
            (StatusCode::NOT_FOUND, "not-found", false)
        }
        SubmissionError::IdentityConflict => (StatusCode::CONFLICT, "identity-conflict", false),
        SubmissionError::CapacityExhausted => {
            (StatusCode::SERVICE_UNAVAILABLE, "capacity-exhausted", true)
        }
        SubmissionError::ConcurrencyExhausted => (
            StatusCode::SERVICE_UNAVAILABLE,
            "concurrency-exhausted",
            true,
        ),
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
