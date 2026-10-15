//! Future typed-context ingress contract. The application installs the extension; headers are not identity.
#![allow(
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
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
use rostfrei::{
    AuthorizationError, CommandAccess, CommandAuthorizer, CommandProcessingService, PrincipalId,
    SecurityScope, TrustedCommandContext, ValidatedCommand,
};
use tower::ServiceExt;
rostfrei::install_macro_support!();

struct Policy {
    deny: AtomicBool,
    seen: Mutex<Vec<(String, Vec<u8>)>>,
}
#[async_trait]
impl CommandAuthorizer for Policy {
    async fn authorize(
        &self,
        caller: &TrustedCommandContext,
        command: &ValidatedCommand,
        _: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.seen.lock().unwrap().push((
            caller.principal().as_str().to_owned(),
            command.original().payload().to_vec(),
        ));
        if self.deny.load(Ordering::SeqCst) {
            Err(AuthorizationError::Denied)
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn typed_http_extension_reaches_service_and_expiry_maps_to_410_only_after_authorization()
-> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy {
        deny: AtomicBool::new(false),
        seen: Mutex::new(Vec::new()),
    });
    let mut processor = CommandProcessor::builder(context()?, store.clone())
        .authorizer(policy.clone())
        .build()?;
    processor.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
    let service: Arc<dyn CommandProcessingService> = Arc::new(processor);
    let caller = TrustedCommandContext::new(
        PrincipalId::new("middleware-verified")?,
        SecurityScope::new("realm")?,
    );
    let command = noncanonical(
        &bus(baseline_processor(
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
        )?)?
        .encode(request("http-context", false)?)?,
    )?;
    let config = rostfrei_http::HttpApiConfig::default()
        .with_trusted_command_envelope_ingress()
        .with_authentication_challenge(HeaderValue::from_static("Bearer realm=\"modular\""))?;
    let bare = rostfrei_http::context_command_router(context()?, service.clone(), config);
    let authenticated = bare.clone().layer(Extension(caller.clone()));
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/contexts/modular/commands/change/schemas/1/envelope")
            .header("content-type", "application/json")
            .header("principal", "spoof-admin")
            .header("tenant", "spoof-tenant")
            .body(Body::from(command.payload().to_vec()))
            .unwrap()
    };
    let unauthenticated = bare.oneshot(request()).await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        unauthenticated.headers()["www-authenticate"],
        "Bearer realm=\"modular\""
    );
    assert!(policy.seen.lock().unwrap().is_empty());
    assert!(
        authenticated
            .clone()
            .oneshot(request())
            .await?
            .status()
            .is_success()
    );
    let OutcomeLookup::Completed(completed) =
        service.lookup(&caller, &command).await.into_result()?
    else {
        return Err("HTTP did not use injected service".into());
    };
    let key = OutcomeKey::new(
        store.authority().clone(),
        context()?.name().clone(),
        OperationId::new(command.operation_id().as_str())?,
    );
    store
        .expire_details(&key, completed.marker().revision())
        .await?;
    policy.deny.store(true, Ordering::SeqCst);
    assert_eq!(
        authenticated.clone().oneshot(request()).await?.status(),
        StatusCode::FORBIDDEN,
        "denied caller must not learn expiry"
    );
    policy.deny.store(false, Ordering::SeqCst);
    let expired = authenticated.oneshot(request()).await?;
    assert_eq!(expired.status(), StatusCode::GONE);
    let bytes = expired.into_body().collect().await?.to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(body["code"], "rostfrei.command.completed-details-expired");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    for (principal, bytes) in policy.seen.lock().unwrap().iter() {
        assert_eq!(principal, "middleware-verified");
        assert_eq!(bytes, command.payload());
    }
    Ok(())
}

struct DetailFault(Option<rostfrei_core::DetailReadError>);
#[async_trait]
impl rostfrei_core::OutcomeDetailReader for DetailFault {
    async fn get(
        &self,
        _: &rostfrei_core::OutcomeDetailReference,
    ) -> Result<rostfrei_core::DetailRead, rostfrei_core::DetailReadError> {
        self.0
            .clone()
            .map_or(Ok(rostfrei_core::DetailRead::AbsentHere), Err)
    }
}

#[tokio::test]
async fn unavailable_missing_and_forbidden_completed_details_are_typed_http_errors() -> TestResult {
    for (fault, status, code) in [
        (
            Some(rostfrei_core::DetailReadError::Unavailable),
            StatusCode::SERVICE_UNAVAILABLE,
            "rostfrei.command.completed-details-unavailable",
        ),
        (
            None,
            StatusCode::INTERNAL_SERVER_ERROR,
            "rostfrei.command.completed-details-blocked",
        ),
        (
            Some(rostfrei_core::DetailReadError::Forbidden),
            StatusCode::INTERNAL_SERVER_ERROR,
            "rostfrei.command.completed-details-blocked",
        ),
    ] {
        let store = InMemoryEventStore::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut processor = CommandProcessor::builder(context()?, Arc::new(store.clone()))
            .authorizer(Arc::new(rostfrei::AllowAllCommands))
            .build()?;
        processor.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
        let caller =
            TrustedCommandContext::new(PrincipalId::new("verified")?, SecurityScope::new("scope")?);
        let command = bus(baseline_processor(
            Arc::new(InMemoryEventStore::new()),
            calls.clone(),
        )?)?
        .encode(request("detail-error", true)?)?;
        CommandProcessingService::process(&processor, &caller, &command)
            .await
            .into_result()?;
        let resolver = store.with_detail_readers(Arc::new(DetailFault(fault)), None);
        let service = CommandProcessor::builder(context()?, Arc::new(resolver))
            .authorizer(Arc::new(rostfrei::AllowAllCommands))
            .build()?;
        let router = rostfrei_http::context_command_router(
            context()?,
            Arc::new(service),
            rostfrei_http::HttpApiConfig::default().with_trusted_command_envelope_ingress(),
        )
        .layer(Extension(caller));
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/contexts/modular/commands/change/schemas/1/envelope")
                    .header("content-type", "application/json")
                    .body(Body::from(command.payload().to_vec()))?,
            )
            .await?;
        assert_eq!(response.status(), status);
        let bytes = response.into_body().collect().await?.to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes)?;
        assert_eq!(body["code"], code);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}
