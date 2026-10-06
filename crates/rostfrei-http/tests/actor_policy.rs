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
    AuthorizationError, CommandAccess, CommandAuthorizer, CommandProcessingService, PrincipalId,
    TrustedCommandContext, ValidatedCommand,
};
use tower::ServiceExt;
rostfrei_runtime::install_macro_support!();

type Row = (String, Option<ActorRef>, Vec<u8>);
#[derive(Default)]
struct Policy(Mutex<Vec<Row>>);
#[async_trait]
impl CommandAuthorizer for Policy {
    async fn authorize(
        &self,
        caller: &TrustedCommandContext,
        command: &ValidatedCommand,
        _: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.0.lock().unwrap().push((
            caller.principal().as_str().to_owned(),
            command.original().actor().cloned(),
            command.original().payload().to_vec(),
        ));
        if !caller.principal().as_str().starts_with("verified-")
            || command.original().actor().map(ActorRef::id) != Some("alice")
        {
            return Err(AuthorizationError::Denied);
        }
        Ok(())
    }
}

#[tokio::test]
async fn envelope_actor_reaches_http_policy_without_becoming_an_authenticated_caller() -> TestResult
{
    let store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy::default());
    let mut processor = CommandProcessor::builder(context()?, store.clone())
        .authorizer(policy.clone())
        .build()?;
    processor.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
    let service: Arc<dyn CommandProcessingService> = Arc::new(processor);
    let bus = bus(baseline_processor(store.clone(), calls.clone())?)?;
    let original = noncanonical(
        &bus.encode(request("http-actor", false)?.with_actor(ActorRef::new("people", "alice")?))?,
    )?;
    let forged =
        bus.encode(request("http-forged", false)?.with_actor(ActorRef::new("people", "admin")?))?;
    let router = rostfrei_http::context_command_router(
        context()?,
        service.clone(),
        rostfrei_http::HttpApiConfig::default()
            .with_trusted_command_envelope_ingress()
            .with_authentication_challenge(axum::http::HeaderValue::from_static(
                "Bearer realm=\"actor\"",
            ))?,
    );
    for (principal, command, status) in [
        (None, &original, StatusCode::UNAUTHORIZED),
        (Some("alice"), &original, StatusCode::FORBIDDEN),
        (Some("verified-a"), &forged, StatusCode::FORBIDDEN),
        (Some("verified-a"), &original, StatusCode::OK),
        (Some("verified-b"), &original, StatusCode::OK),
    ] {
        let mut app = router.clone();
        if let Some(principal) = principal {
            app = app.layer(Extension(TrustedCommandContext::new(
                PrincipalId::new(principal)?,
                SecurityScope::new("scope")?,
            )));
        }
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/contexts/modular/commands/change/schemas/1/envelope")
                    .header("content-type", "application/json")
                    .header("principal", "verified-spoof")
                    .body(Body::from(command.payload().to_vec()))?,
            )
            .await?;
        assert_eq!(response.status(), status);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let history = store.load(&stream()?).await?;
    assert_eq!(history.len(), 1);
    assert_eq!(
        history.first().and_then(RecordedEvent::actor),
        original.actor()
    );
    let rows = policy.0.lock().unwrap();
    assert_eq!(rows.len(), 5);
    for (principal, actor, bytes) in rows.iter() {
        assert_ne!(principal, "verified-spoof");
        let expected = if actor.as_ref().map(ActorRef::id) == Some("admin") {
            &forged
        } else {
            &original
        };
        assert_eq!(actor.as_ref(), expected.actor());
        assert_eq!(bytes, expected.payload());
    }
    drop(rows);
    Ok(())
}
