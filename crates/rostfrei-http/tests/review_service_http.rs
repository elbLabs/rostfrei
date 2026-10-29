//! Independent injected-service admission and transport mapping controls.
#![allow(
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    clippy::indexing_slicing
)]
#[path = "../../rostfrei/tests/modular_support/mod.rs"]
mod modular_support;
use async_trait::async_trait;
use axum::{
    Extension,
    body::Body,
    http::{HeaderValue, Request, StatusCode},
};
use http_body_util::BodyExt;
use modular_support::*;
use rostfrei_runtime::{
    AllowAllCommands, AttemptId, AuditDelivery, AuthorizationError, CommandAccess,
    CommandAuthorizer, CommandProcessingService, PrincipalId, ProcessedCommand, ServiceReport,
    TrustedCommandContext, ValidatedCommand,
};
use tower::ServiceExt;
rostfrei_runtime::install_macro_support!();

fn caller(scope: &str) -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new("verified")?,
        SecurityScope::new(scope)?,
    ))
}
fn encoded(id: &str) -> TestResult<EncodedCommand> {
    noncanonical(
        &bus(baseline_processor(
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
        )?)?
        .encode(request(id, false)?)?,
    )
}
fn request_for(command: &EncodedCommand) -> TestResult<Request<Body>> {
    Ok(Request::builder()
        .method("POST")
        .uri("/contexts/modular/commands/change/schemas/1/envelope")
        .header("content-type", "application/json")
        .body(Body::from(command.payload().to_vec()))?)
}
struct WrongWinner(ProcessedCommand);
#[async_trait]
impl CommandProcessingService for WrongWinner {
    async fn process(
        &self,
        _: &TrustedCommandContext,
        _: &EncodedCommand,
    ) -> ServiceReport<ProcessedCommand> {
        ServiceReport::new(
            Ok(self.0.clone()),
            AttemptId::generate(),
            AuditDelivery::Disabled,
        )
    }
    async fn lookup(
        &self,
        _: &TrustedCommandContext,
        _: &EncodedCommand,
    ) -> ServiceReport<OutcomeLookup<rostfrei_runtime::CommandResponse>> {
        ServiceReport::new(
            Ok(OutcomeLookup::Completed(self.0.outcome().clone())),
            AttemptId::generate(),
            AuditDelivery::Disabled,
        )
    }
}
async fn substituted_winner(scope_only: bool) -> TestResult {
    let mut processor = CommandProcessor::builder(context()?, Arc::new(InMemoryEventStore::new()))
        .authorizer(Arc::new(AllowAllCommands))
        .build()?;
    processor.register::<Change, _>(ChangeHandler(Arc::new(AtomicUsize::new(0))), map_rejection)?;
    let original = encoded("genuine-winner")?;
    let winner = CommandProcessingService::process(&processor, &caller("scope-a")?, &original)
        .await
        .into_result()?;
    let requested = if scope_only {
        original
    } else {
        encoded("not-the-winner")?
    };
    let who = caller(if scope_only { "scope-b" } else { "scope-a" })?;
    let app = rostfrei_http::context_command_router(
        context()?,
        Arc::new(WrongWinner(winner)),
        rostfrei_http::HttpApiConfig::default().with_trusted_command_envelope_ingress(),
    )
    .layer(Extension(who));
    let response = app.oneshot(request_for(&requested)?).await?;
    let status = response.status();
    let body = response.into_body().collect().await?.to_bytes();
    eprintln!(
        "substituted HTTP result, scope_only={scope_only}: {status}, {}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        !status.is_success(),
        "HTTP admitted an intact winner for a different command/scope"
    );
    Ok(())
}
#[tokio::test]
async fn http_refuses_genuine_completion_for_another_operation() -> TestResult {
    substituted_winner(false).await
}
#[tokio::test]
async fn http_refuses_genuine_completion_for_another_scope() -> TestResult {
    substituted_winner(true).await
}

struct FailedPolicy(AuthorizationError);
#[async_trait]
impl CommandAuthorizer for FailedPolicy {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &ValidatedCommand,
        _: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        Err(self.0)
    }
}
#[tokio::test]
async fn all_provider_failure_projections_and_challenges_are_no_store() -> TestResult {
    for (failure, expected, code) in [
        (
            AuthorizationError::Unauthenticated,
            StatusCode::UNAUTHORIZED,
            "rostfrei.command.unauthenticated",
        ),
        (
            AuthorizationError::Denied,
            StatusCode::FORBIDDEN,
            "rostfrei.command.forbidden",
        ),
        (
            AuthorizationError::Unavailable,
            StatusCode::SERVICE_UNAVAILABLE,
            "rostfrei.command.authorization-unavailable",
        ),
        (
            AuthorizationError::InvalidConfiguration,
            StatusCode::INTERNAL_SERVER_ERROR,
            "rostfrei.command.authorization-configuration",
        ),
    ] {
        for challenge in [false, true] {
            let processor =
                CommandProcessor::builder(context()?, Arc::new(InMemoryEventStore::new()))
                    .authorizer(Arc::new(FailedPolicy(failure)))
                    .build()?;
            let mut config =
                rostfrei_http::HttpApiConfig::default().with_trusted_command_envelope_ingress();
            if challenge {
                config = config.with_authentication_challenge(HeaderValue::from_static(
                    "Bearer realm=review",
                ))?;
            }
            let app =
                rostfrei_http::context_command_router(context()?, Arc::new(processor), config)
                    .layer(Extension(caller("scope")?));
            let response = app
                .oneshot(request_for(&encoded("provider-mapping")?)?)
                .await?;
            assert_eq!(
                response.status(),
                if !challenge && expected == StatusCode::UNAUTHORIZED {
                    StatusCode::FORBIDDEN
                } else {
                    expected
                }
            );
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            assert_eq!(
                response.headers().contains_key("www-authenticate"),
                challenge && failure == AuthorizationError::Unauthenticated
            );
            let body: serde_json::Value =
                serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
            assert_eq!(body["code"], code);
        }
    }
    Ok(())
}
