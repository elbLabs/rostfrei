//! Independent pure validation before arbitrary service injection, on both adapters.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
#[path = "../../rostfrei/tests/modular_support/mod.rs"]
mod modular_support;
use async_trait::async_trait;
use axum::{
    Extension,
    body::Body,
    http::{Request, StatusCode},
};
use modular_support::*;
use rostfrei_runtime::{
    AllowAllCommands, AttemptId, AuditDelivery, CommandProcessingService, PrincipalId,
    ProcessedCommand, ServiceReport, TrustedCommandContext,
};
use tower::ServiceExt;
rostfrei_runtime::install_macro_support!();

struct Injected {
    result: ProcessedCommand,
    seen: Mutex<Vec<Vec<u8>>>,
}
#[async_trait]
impl CommandProcessingService for Injected {
    async fn process(
        &self,
        _: &TrustedCommandContext,
        command: &EncodedCommand,
    ) -> ServiceReport<ProcessedCommand> {
        self.seen.lock().unwrap().push(command.payload().to_vec());
        ServiceReport::new(
            Ok(self.result.clone()),
            AttemptId::generate(),
            AuditDelivery::Disabled,
        )
    }
    #[allow(clippy::panic, reason = "test trap detects an unexpected adapter call")]
    async fn lookup(
        &self,
        _: &TrustedCommandContext,
        _: &EncodedCommand,
    ) -> ServiceReport<OutcomeLookup<rostfrei_runtime::CommandResponse>> {
        panic!("adapter unexpectedly performed service lookup")
    }
}
#[tokio::test]
async fn original_envelope_is_validated_before_injected_service_for_local_and_http() -> TestResult {
    let calls = Arc::new(AtomicUsize::new(0));
    let command = noncanonical(
        &bus(baseline_processor(
            Arc::new(InMemoryEventStore::new()),
            calls.clone(),
        )?)?
        .encode(request("pre-injection-validation", false)?)?,
    )?;
    let caller =
        TrustedCommandContext::new(PrincipalId::new("verified")?, SecurityScope::new("scope")?);
    let mut real = CommandProcessor::builder(context()?, Arc::new(InMemoryEventStore::new()))
        .authorizer(Arc::new(AllowAllCommands))
        .build()?;
    real.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
    let winner = CommandProcessingService::process(&real, &caller, &command)
        .await
        .into_result()?;
    let injected = Arc::new(Injected {
        result: winner,
        seen: Mutex::new(Vec::new()),
    });
    let local = InMemoryMessagingAdapter::with_service(injected.clone());
    let app = rostfrei_http::context_command_router(
        context()?,
        injected.clone(),
        rostfrei_http::HttpApiConfig::default().with_trusted_command_envelope_ingress(),
    )
    .layer(Extension(caller.clone()));
    let request = |bytes: Vec<u8>| {
        Request::builder()
            .method("POST")
            .uri("/contexts/modular/commands/change/schemas/1/envelope")
            .header("content-type", "application/json")
            .body(Body::from(bytes))
    };
    let mut json: serde_json::Value = serde_json::from_slice(command.payload())?;
    *json
        .get_mut("message_id")
        .ok_or("missing envelope message identity")? = "noncanonical-identity".into();
    let bytes = serde_json::to_vec_pretty(&json)?;
    let inconsistent = EncodedCommand::from_delivery(
        command.address().clone(),
        rostfrei_runtime::MessageId::new("noncanonical-identity")?,
        bytes.clone(),
    )?;
    assert_eq!(
        local
            .dispatch_as(&caller, inconsistent)
            .await
            .unwrap_err()
            .kind(),
        rostfrei_runtime::CommandBusErrorKind::InvalidMessage
    );
    let response = app.clone().oneshot(request(bytes)?).await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.headers().get("cache-control").unwrap(),
        "private, no-store"
    );
    assert!(injected.seen.lock().unwrap().is_empty());
    assert!(local.command_messages().await.is_empty());
    // A valid noncanonical envelope reaches the independent service unchanged on both paths.
    local.dispatch_as(&caller, command.clone()).await?;
    assert_eq!(
        app.oneshot(request(command.payload().to_vec())?)
            .await?
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        *injected.seen.lock().unwrap(),
        vec![command.payload().to_vec(), command.payload().to_vec()]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}
