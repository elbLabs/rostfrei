use axum::Extension;
use rostfrei::{
    AuthorizationError, BoundedContext, CommandProcessingService, CommandProcessorError,
    TrustedCommandContext,
};

use super::{
    Arc, Bytes, CommandBusError, DefaultBodyLimit, EncodedCommand, HeaderMap, HeaderValue,
    HttpApiConfig, IDEMPOTENCY_KEY_HEADER, Path, Response, RoutedCommand, Router, State,
    StatusCode, add_authentication_challenge, add_private_no_store, bad_request, command_bus_error,
    command_response, error_response, idempotency_key, middleware, not_found,
    optional_single_header, post,
};

const ENVELOPE_ROUTE: &str =
    "/contexts/{context}/commands/{command}/schemas/{schema_version}/envelope";

#[derive(Clone)]
struct ContextState {
    context: BoundedContext,
    service: Arc<dyn CommandProcessingService>,
    config: HttpApiConfig,
}

/// Local processing ingress. Application authentication middleware supplies the typed
/// extension; request headers never establish principal or security scope.
pub fn context_command_router(
    context: BoundedContext,
    service: Arc<dyn CommandProcessingService>,
    config: HttpApiConfig,
) -> Router {
    Router::new()
        .route(ENVELOPE_ROUTE, post(submit))
        .layer(DefaultBodyLimit::max(
            config.maximum_command_payload_bytes(),
        ))
        .layer(middleware::map_response(add_private_no_store))
        .with_state(ContextState {
            context,
            service,
            config,
        })
}

async fn submit(
    State(state): State<ContextState>,
    Path((context, command, schema)): Path<(String, String, u32)>,
    caller: Option<Extension<TrustedCommandContext>>,
    headers: HeaderMap,
    payload: Bytes,
) -> Response {
    let challenge = state.config.authentication_challenge();
    let Some(Extension(caller)) = caller else {
        return authorization_error(AuthorizationError::Unauthenticated, challenge);
    };
    if !state.config.trusted_command_envelope_ingress {
        return not_found("Command envelope ingress is disabled.");
    }
    if context != state.context.name().as_str() {
        return not_found("Command context is not configured.");
    }
    if !matches!(
        optional_single_header(&headers, "content-type"),
        Ok(Some("application/json"))
    ) {
        return error_response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "rostfrei.http.invalid-content-type",
            "Command envelopes require exactly one application/json content-type.",
        );
    }
    let envelope: rostfrei::CommandEnvelope<RoutedCommand> = match serde_json::from_slice(&payload)
    {
        Ok(envelope) => envelope,
        Err(error) => {
            return bad_request("rostfrei.http.invalid-command-envelope", error.to_string());
        }
    };
    if envelope.payload().command() != command || envelope.payload().schema_version() != schema {
        return bad_request(
            "rostfrei.http.command-envelope-identity",
            "Envelope and route differ.",
        );
    }
    if headers.contains_key(IDEMPOTENCY_KEY_HEADER) {
        match idempotency_key(&headers) {
            Ok(key) if key.as_str() == envelope.operation_id().as_str() => {}
            Ok(_) => {
                return bad_request(
                    "rostfrei.http.command-envelope-identity",
                    "Envelope and idempotency key differ.",
                );
            }
            Err(error) => return error.into_response(),
        }
    }
    let encoded = state
        .context
        .command_address(&command)
        .map_err(|error| error.to_string())
        .and_then(|address| {
            EncodedCommand::from_delivery(address, envelope.message_id().clone(), payload.to_vec())
                .map_err(|e| e.to_string())
        });
    let encoded = match encoded {
        Ok(command) => command,
        Err(error) => return bad_request("rostfrei.http.invalid-command-envelope", error),
    };
    if let Err(error) = encoded.validate() {
        return processor_error(&error, challenge);
    }
    match state.service.process(&caller, &encoded).await.into_result() {
        Ok(processed) => match processed.admit(&caller, &encoded) {
            Ok(response) => command_response(response.clone(), challenge),
            Err(error) => processor_error(&error, challenge),
        },
        Err(error) => processor_error(&error, challenge),
    }
}

fn processor_error(error: &CommandProcessorError, challenge: Option<&HeaderValue>) -> Response {
    if let Some(error) = error.authorization_error() {
        return authorization_error(*error, challenge);
    }
    match error.kind() {
        rostfrei::CommandProcessorErrorKind::IdentityConflict => error_response(
            StatusCode::CONFLICT,
            "rostfrei.command.identity-conflict",
            error.to_string(),
        ),
        rostfrei::CommandProcessorErrorKind::InvalidMessage => {
            bad_request("rostfrei.http.invalid-command-envelope", error.to_string())
        }
        _ => command_bus_error(&CommandBusError::from_processor(error)),
    }
}

fn authorization_error(error: AuthorizationError, challenge: Option<&HeaderValue>) -> Response {
    let (status, code) = match error {
        AuthorizationError::Unauthenticated => (
            if challenge.is_some() {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::FORBIDDEN
            },
            "rostfrei.command.unauthenticated",
        ),
        AuthorizationError::Denied => (StatusCode::FORBIDDEN, "rostfrei.command.forbidden"),
        AuthorizationError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "rostfrei.command.authorization-unavailable",
        ),
        AuthorizationError::InvalidConfiguration => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "rostfrei.command.authorization-configuration",
        ),
    };
    let mut response = error_response(status, code, error.to_string());
    add_authentication_challenge(&mut response, status, challenge);
    response
}
