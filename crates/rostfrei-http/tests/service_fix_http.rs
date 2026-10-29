#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
#[path = "../../rostfrei/tests/modular_support/mod.rs"]
mod modular_support;
#[path = "../../rostfrei/tests/service_fix_support/mod.rs"]
mod service_fix_support;
use axum::{
    Extension,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use modular_support::*;
use service_fix_support::*;
use tower::ServiceExt;
rostfrei_runtime::install_macro_support!();

fn app(
    service: Arc<dyn CommandProcessingService>,
    who: TrustedCommandContext,
) -> TestResult<axum::Router> {
    Ok(rostfrei_http::context_command_router(
        context()?,
        service,
        rostfrei_http::HttpApiConfig::default().with_trusted_command_envelope_ingress(),
    )
    .layer(Extension(who)))
}
fn request_for(command: &EncodedCommand) -> TestResult<Request<Body>> {
    Ok(Request::builder()
        .method("POST")
        .uri("/contexts/modular/commands/change/schemas/1/envelope")
        .header("content-type", "application/json")
        .body(Body::from(command.payload().to_vec()))?)
}
async fn assert_blocked(response: axum::response::Response) -> TestResult {
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .ok_or("missing no-store")?,
        "private, no-store"
    );
    let body: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    assert_eq!(
        body.get("code"),
        Some(&serde_json::json!(
            "rostfrei.command.completed-details-blocked"
        ))
    );
    assert!(body.get("status").is_none());
    Ok(())
}
#[tokio::test]
async fn http_refuses_same_request_mapped_response_with_opposite_permanent_decision() -> TestResult
{
    for reject in [false, true] {
        let command = encoded("wrong-decision", reject)?;
        let who = caller()?;
        let calls = Arc::new(AtomicUsize::new(0));
        let service = processor(Arc::new(InMemoryEventStore::new()), calls.clone())?;
        let winner = CommandProcessingService::process(&service, &who, &command)
            .await
            .into_result()?;
        let response = opposite_response(winner.outcome().details().available()?)?;
        command.validate_response(&response)?;
        assert_blocked(
            app(
                Arc::new(Substitute(substitute_response(&winner, response)?)),
                who,
            )?
            .oneshot(request_for(&command)?)
            .await?,
        )
        .await?;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}
#[tokio::test]
async fn http_commit_and_initial_corrupt_recovery_have_the_same_typed_error() -> TestResult {
    let store = InMemoryEventStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let command = encoded("commit-corrupt-http", false)?;
    let service = processor(
        Arc::new(
            store
                .clone()
                .with_detail_readers(Arc::new(FaultReader(DetailReadError::Corrupt)), None),
        ),
        calls.clone(),
    )?;
    let router = app(Arc::new(service), caller()?)?;
    for _ in 0..2 {
        assert_blocked(router.clone().oneshot(request_for(&command)?).await?).await?;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.load(&stream()?).await?.len(), 1);
    Ok(())
}
