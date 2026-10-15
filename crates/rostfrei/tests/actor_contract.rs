//! Proposed actor API acceptance, ACT-02..09. API gaps on 010f4b6; no product stubs.
#![allow(
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    clippy::indexing_slicing
)]
mod modular_support;
use async_trait::async_trait;
use modular_support::*;
use rostfrei::{
    ActorRef, AllowAllCommands, AuthorizationError, CommandAccess, CommandAuthorizer,
    CommandProcessingService, CommandProcessorErrorKind, CompletionOrigin, DynamicCommandRequest,
    PrincipalId, TrustedCommandContext, ValidatedCommand,
};
use rostfrei_messaging_core::{
    CommandEnvelope, CorrelationId, EnvelopeContext, IntegrationEventEnvelope, MessageId,
    MessageTimestamp, SchemaVersion,
};
rostfrei::install_macro_support!();

fn actor(id: &str) -> ActorRef {
    ActorRef::new("people", id).unwrap()
}
fn caller(id: &str) -> TrustedCommandContext {
    TrustedCommandContext::new(
        PrincipalId::new(id).unwrap(),
        SecurityScope::new("scope").unwrap(),
    )
}
fn command(
    id: &str,
    amount: i64,
    reject: bool,
    attribution: Option<ActorRef>,
) -> TestResult<EncodedCommand> {
    let mut request = CommandRequest::new(OperationId::new(id)?, Change { amount, reject })
        .with_created_at(MessageTimestamp::from_unix_milliseconds(42)?);
    if let Some(actor) = attribution {
        request = request.with_actor(actor);
    }
    bus(baseline_processor(
        Arc::new(InMemoryEventStore::new()),
        Arc::new(AtomicUsize::new(0)),
    )?)?
    .encode(request)
    .map_err(Into::into)
}
type Seen = Arc<Mutex<Vec<Option<ActorRef>>>>;
struct Observe {
    seen: Seen,
    race: Option<Arc<InMemoryEventStore>>,
}
#[async_trait]
impl CommandHandler<Change> for Observe {
    type Rejection = String;
    async fn handle(
        &self,
        command: &Change,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        let first = {
            let mut seen = self.seen.lock().unwrap();
            seen.push(execution.metadata().actor().cloned());
            seen.len() == 1
        };
        let mut counter = execution.load::<Counter>("one").await?;
        if first && let Some(store) = &self.race {
            let op = OperationId::new("competing-write").unwrap();
            let commit = derive_commit_id(&stream().unwrap(), &op);
            store
                .append(
                    &stream().unwrap(),
                    ExpectedVersion::NoStream,
                    EventBatch::new(
                        commit.clone(),
                        op,
                        ContentFingerprint::digest("competitor"),
                        vec![
                            NewEvent::new(derive_event_id(&commit, 0), "added", 2, b"10".to_vec())
                                .unwrap(),
                        ],
                    )
                    .unwrap(),
                )
                .await?;
        }
        if command.reject {
            return Ok(CommandDecision::Rejected("refused".to_owned()));
        }
        if command.amount != 0 {
            counter.raise(Added(command.amount));
        }
        Ok(CommandDecision::Accepted)
    }
}
fn processor(
    store: Arc<InMemoryEventStore>,
    seen: Seen,
    policy: Arc<dyn CommandAuthorizer>,
    race: bool,
) -> TestResult<Box<dyn CommandProcessingService>> {
    let mut service = CommandProcessor::builder(context()?, store.clone())
        .authorizer(policy)
        .build()?;
    service.register::<Change, _>(
        Observe {
            seen,
            race: race.then_some(store),
        },
        map_rejection,
    )?;
    Ok(Box::new(service))
}

#[test]
fn actor_ref_validates_bytes_without_provider_or_normalization_rules() -> TestResult {
    let value = ActorRef::new("Org:/A*", "User α: stable / 001")?;
    let core: rostfrei_core::ActorRef = value.clone();
    let shared: rostfrei_messaging_core::ActorRef = core;
    assert_eq!(shared.namespace(), "Org:/A*");
    assert_eq!(shared.id(), "User α: stable / 001");
    assert_eq!(
        serde_json::to_value(&shared)?,
        serde_json::json!({"namespace":"Org:/A*", "id":"User α: stable / 001"})
    );
    assert_eq!(
        serde_json::from_value::<ActorRef>(serde_json::to_value(&shared)?)?,
        shared
    );
    assert_ne!(actor("Alice"), actor("alice"));
    assert!(ActorRef::new("n".repeat(128), "é".repeat(128)).is_ok());
    for (namespace, id) in [
        ("".to_owned(), "id".to_owned()),
        ("n".repeat(129), "id".to_owned()),
        ("n".to_owned(), "".to_owned()),
        ("n".to_owned(), "é".repeat(129)),
        (" n".to_owned(), "id".to_owned()),
        ("n".to_owned(), "id ".to_owned()),
        ("n\n".to_owned(), "id".to_owned()),
        ("n".to_owned(), "a\0b".to_owned()),
    ] {
        assert!(ActorRef::new(namespace.clone(), id.clone()).is_err());
        assert!(
            serde_json::from_value::<ActorRef>(serde_json::json!({"namespace":namespace,"id":id}))
                .is_err()
        );
    }
    for invalid in [
        serde_json::json!({"namespace":"n"}),
        serde_json::json!({"namespace":"n","id":3}),
        serde_json::json!({"namespace":"n","id":"x","roles":["admin"]}),
    ] {
        assert!(serde_json::from_value::<ActorRef>(invalid).is_err());
    }
    assert!(serde_json::from_str::<ActorRef>(r#"{"namespace":"n","id":"a","id":"b"}"#).is_err());
    Ok(())
}

#[test]
fn command_and_integration_envelopes_roundtrip_legacy_absence_and_reject_corrupt_actor()
-> TestResult {
    let context = EnvelopeContext::new(
        MessageId::new("m")?,
        SchemaVersion::new(1)?,
        CorrelationId::new("c")?,
        None,
    );
    let old_command = CommandEnvelope::new(
        context.clone(),
        rostfrei_messaging_core::OperationId::new("o")?,
        MessageTimestamp::from_unix_milliseconds(1)?,
        serde_json::json!({"domain":"payload"}),
    )?;
    let old_event = IntegrationEventEnvelope::new(
        context,
        MessageTimestamp::from_unix_milliseconds(1)?,
        serde_json::json!({"domain":"payload"}),
    )?;
    for (command, old, attributed) in [
        (
            true,
            serde_json::to_value(&old_command)?,
            serde_json::to_value(old_command.with_actor(actor("alice"))?)?,
        ),
        (
            false,
            serde_json::to_value(&old_event)?,
            serde_json::to_value(old_event.with_actor(actor("alice"))?)?,
        ),
    ] {
        assert!(old.get("actor").is_none());
        assert!(old.get("metadata_version").is_none());
        let decode = |wire: serde_json::Value| -> Result<Option<ActorRef>, serde_json::Error> {
            if command {
                serde_json::from_value::<CommandEnvelope<serde_json::Value>>(wire)
                    .map(|v| v.actor().cloned())
            } else {
                serde_json::from_value::<IntegrationEventEnvelope<serde_json::Value>>(wire)
                    .map(|v| v.actor().cloned())
            }
        };
        assert_eq!(decode(old.clone())?, None);
        let mut full = old;
        full["payload"] = serde_json::json!("");
        let remaining =
            rostfrei_messaging_core::MAX_ENVELOPE_BYTES - serde_json::to_vec(&full)?.len();
        full["payload"] = serde_json::json!("x".repeat(remaining));
        if command {
            let full: CommandEnvelope<serde_json::Value> = serde_json::from_value(full)?;
            assert!(full.with_actor(actor("alice")).is_err());
        } else {
            let full: IntegrationEventEnvelope<serde_json::Value> = serde_json::from_value(full)?;
            assert!(full.with_actor(actor("alice")).is_err());
        }
        assert_eq!(attributed["metadata_version"], 2);
        assert_eq!(decode(attributed.clone())?, Some(actor("alice")));
        for bad in [
            serde_json::Value::Null,
            serde_json::json!({"namespace":"", "id":"a"}),
            serde_json::json!("alice"),
        ] {
            let mut wire = attributed.clone();
            wire["actor"] = bad;
            assert!(decode(wire).is_err());
        }
        for field in ["actor", "metadata_version"] {
            let mut wire = attributed.clone();
            wire.as_object_mut().unwrap().remove(field);
            assert!(decode(wire).is_err(), "stripped {field}");
        }
    }
    Ok(())
}

#[tokio::test]
async fn accepted_noevent_and_rejected_winners_keep_actor_after_expiry() -> TestResult {
    for attribution in [None, Some(actor("alice"))] {
        for (amount, reject) in [(7, false), (0, false), (7, true)] {
            let store = Arc::new(InMemoryEventStore::new());
            let _: &dyn rostfrei::ExecutionStore = store.as_ref();
            let seen = Seen::default();
            let service = processor(
                store.clone(),
                seen.clone(),
                Arc::new(AllowAllCommands),
                false,
            )?;
            let original = noncanonical(&command(
                "durable-actor",
                amount,
                reject,
                attribution.clone(),
            )?)?;
            let first = service
                .process(&caller("worker-a"), &original)
                .await
                .into_result()?;
            assert_eq!(first.origin(), CompletionOrigin::Committed);
            assert_eq!(first.outcome().marker().actor(), attribution.as_ref());
            assert_eq!(
                first.outcome().marker().integrity_receipt().actor(),
                attribution.as_ref()
            );
            assert_eq!(
                matches!(
                    first.outcome().details().available()?.outcome(),
                    CommandResponseOutcome::Rejected(_)
                ),
                reject
            );
            let history = store.load(&stream()?).await?;
            assert_eq!(history.len(), usize::from(amount != 0 && !reject));
            for event in &history {
                assert_eq!(event.actor(), attribution.as_ref());
            }
            assert_eq!(
                service
                    .process(&caller("worker-b"), &original)
                    .await
                    .into_result()?
                    .outcome(),
                first.outcome()
            );
            let direct = CommandExecutor::new(store.clone())
                .execute_completed_with_rejection_codec(
                    &Observe {
                        seen: seen.clone(),
                        race: None,
                    },
                    original
                        .validated_execution_metadata()?
                        .with_security_scope(caller("direct-worker").security_scope().clone()),
                    &Change { amount, reject },
                    &JsonRejectionCodec::new("unused-on-completed-replay", 1),
                )
                .await?;
            assert!(direct.is_exact_replay());
            assert_eq!(direct.outcome().marker(), first.outcome().marker());
            let marker = first.outcome().marker();
            store
                .expire_details(marker.detail_reference().key(), marker.revision())
                .await?;
            let OutcomeLookup::Completed(expired) = service
                .lookup(&caller("worker-c"), &original)
                .await
                .into_result()?
            else {
                return Err("lost winner".into());
            };
            assert_eq!(expired.marker(), marker);
            assert!(matches!(expired.details(), OutcomeDetails::Expired));
            assert_eq!(store.load(&stream()?).await?, history);
            assert_eq!(*seen.lock().unwrap(), [attribution.clone()]);
        }
    }
    Ok(())
}

#[tokio::test]
async fn conflict_retry_and_replay_do_not_drift_actor_to_current_caller() -> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let seen = Seen::default();
    let service = processor(
        store.clone(),
        seen.clone(),
        Arc::new(AllowAllCommands),
        true,
    )?;
    let original = command("retry-actor", 7, false, Some(actor("alice")))?;
    let first = service
        .process(&caller("queue-worker"), &original)
        .await
        .into_result()?;
    assert_eq!(
        *seen.lock().unwrap(),
        [Some(actor("alice")), Some(actor("alice"))]
    );
    let replay = service
        .process(&caller("support-worker"), &original)
        .await
        .into_result()?;
    assert_eq!(replay.origin(), CompletionOrigin::Replayed);
    assert_eq!(replay.outcome(), first.outcome());
    assert_eq!(seen.lock().unwrap().len(), 2);
    let history = store.load(&stream()?).await?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].actor(), None);
    assert_eq!(history[1].actor(), Some(&actor("alice")));
    assert_eq!(history[1].correlation_id(), Some(original.correlation_id()));
    Ok(())
}

#[tokio::test]
async fn same_operation_actor_change_addition_or_removal_conflicts_without_execution() -> TestResult
{
    for (original_actor, changed_actor) in [
        (Some(actor("alice")), Some(actor("bob"))),
        (
            Some(actor("alice")),
            Some(ActorRef::new("services", "alice")?),
        ),
        (Some(actor("alice")), None),
        (None, Some(actor("alice"))),
    ] {
        let store = Arc::new(InMemoryEventStore::new());
        let seen = Seen::default();
        let service = processor(
            store.clone(),
            seen.clone(),
            Arc::new(AllowAllCommands),
            false,
        )?;
        let original = command("immutable-actor", 7, false, original_actor)?;
        let changed = command("immutable-actor", 7, false, changed_actor)?;
        assert_eq!(original.operation_id(), changed.operation_id());
        let first = service
            .process(&caller("authorized"), &original)
            .await
            .into_result()?;
        let before = store.load(&stream()?).await?;
        for expired in [false, true] {
            if expired {
                let m = first.outcome().marker();
                store
                    .expire_details(m.detail_reference().key(), m.revision())
                    .await?;
            }
            assert_eq!(
                service
                    .process(&caller("other-authorized"), &changed)
                    .await
                    .into_result()
                    .unwrap_err()
                    .kind(),
                CommandProcessorErrorKind::IdentityConflict
            );
            assert_eq!(
                service
                    .lookup(&caller("other-authorized"), &changed)
                    .await
                    .into_result()
                    .unwrap_err()
                    .kind(),
                CommandProcessorErrorKind::IdentityConflict
            );
            let OutcomeLookup::Completed(winner) = store
                .lookup_outcome(first.outcome().marker().detail_reference().key())
                .await?
            else {
                return Err("lost winner".into());
            };
            assert_eq!(winner.marker(), first.outcome().marker());
            assert_eq!(store.load(&stream()?).await?, before);
        }
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
    Ok(())
}

type PolicyRow = (CommandAccess, String, Option<ActorRef>, Vec<u8>);
#[derive(Default)]
struct AttributionPolicy {
    seen: Mutex<Vec<PolicyRow>>,
    deny_reads: AtomicBool,
}
#[async_trait]
impl CommandAuthorizer for AttributionPolicy {
    async fn authorize(
        &self,
        caller: &TrustedCommandContext,
        command: &ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.seen.lock().unwrap().push((
            access,
            caller.principal().as_str().to_owned(),
            command.original().actor().cloned(),
            command.original().payload().to_vec(),
        ));
        // Fixture trust rule: only this authenticated service may carry this attribution.
        if caller.principal().as_str() != "verified-producer"
            || command.original().actor() != Some(&actor("alice"))
            || (access == CommandAccess::ReadOutcome && self.deny_reads.load(Ordering::SeqCst))
        {
            return Err(AuthorizationError::Denied);
        }
        Ok(())
    }
}
#[tokio::test]
async fn actor_claim_is_observable_but_never_authenticates_or_overrides_policy() -> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let seen = Seen::default();
    let policy = Arc::new(AttributionPolicy::default());
    let service = processor(store.clone(), seen.clone(), policy.clone(), false)?;
    let valid = noncanonical(&command("policy-actor", 7, false, Some(actor("alice")))?)?;
    for (who, request) in [
        (caller("alice"), valid.clone()),
        (caller("untrusted"), valid.clone()),
        (
            caller("verified-producer"),
            command("policy-actor", 7, false, Some(actor("admin")))?,
        ),
    ] {
        assert_eq!(
            service
                .process(&who, &request)
                .await
                .into_result()
                .unwrap_err()
                .authorization_error(),
            Some(&AuthorizationError::Denied)
        );
    }
    assert!(seen.lock().unwrap().is_empty());
    assert!(store.load(&stream()?).await?.is_empty());
    let first = service
        .process(&caller("verified-producer"), &valid)
        .await
        .into_result()?;
    policy.deny_reads.store(true, Ordering::SeqCst);
    assert_eq!(
        service
            .process(&caller("verified-producer"), &valid)
            .await
            .into_result()
            .unwrap_err()
            .authorization_error(),
        Some(&AuthorizationError::Denied)
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
    let rows = policy.seen.lock().unwrap();
    assert_eq!(
        rows.iter().map(|r| r.0).collect::<Vec<_>>(),
        [
            CommandAccess::ReadOutcome,
            CommandAccess::ReadOutcome,
            CommandAccess::ReadOutcome,
            CommandAccess::ReadOutcome,
            CommandAccess::Execute,
            CommandAccess::ReadOutcome
        ]
    );
    assert_eq!(rows[0].1, "alice");
    assert_eq!(rows[1].1, "untrusted");
    assert_eq!(rows[4].1, "verified-producer");
    assert_eq!(rows[4].2, Some(actor("alice")));
    assert_eq!(rows[4].3, valid.payload());
    assert_eq!(first.outcome().marker().actor(), Some(&actor("alice")));
    Ok(())
}

#[test]
fn typed_dynamic_and_delivery_paths_bind_actor_without_changing_operation_namespace() -> TestResult
{
    let original = command("hash-actor", 7, false, Some(actor("alice")))?;
    let bus = bus(baseline_processor(
        Arc::new(InMemoryEventStore::new()),
        Arc::new(AtomicUsize::new(0)),
    )?)?;
    let dynamic = bus.encode_dynamic(
        DynamicCommandRequest::new(
            OperationId::new("hash-actor")?,
            "change",
            1,
            serde_json::json!({"amount":7,"reject":false}),
        )?
        .with_created_at(MessageTimestamp::from_unix_milliseconds(42)?)
        .with_actor(actor("alice")),
    )?;
    assert_eq!(original, dynamic);
    assert_eq!(original.actor(), Some(&actor("alice")));
    assert_eq!(
        original.validated_execution_metadata()?.actor(),
        original.actor()
    );
    assert_eq!(
        original
            .validated_execution_metadata()?
            .outcome_evidence()
            .unwrap()
            .fingerprint_version,
        3
    );
    let old = command("hash-actor", 7, false, None)?;
    assert_eq!(
        old.validated_execution_metadata()?
            .outcome_evidence()
            .unwrap()
            .fingerprint_version,
        2
    );
    assert_eq!(old.operation_id(), original.operation_id());
    assert_ne!(old.fingerprint(), original.fingerprint());
    let refusal = rostfrei::CommandResponse::rejected(
        rostfrei::command_response_message_id(original.message_id())?,
        original.message_id().clone(),
        original.address().clone(),
        original.operation_id().clone(),
        original.correlation_id().clone(),
        map_rejection(&"legacy fixture".to_owned())?,
    )?;
    assert!(
        original.legacy_rejection_transaction(&refusal).is_err(),
        "historical import must not invent attributed history"
    );
    for replacement in [serde_json::to_value(actor("bob"))?, serde_json::Value::Null] {
        let mut wire: serde_json::Value = serde_json::from_slice(original.payload())?;
        wire["actor"] = replacement;
        assert!(
            EncodedCommand::from_delivery(
                original.address().clone(),
                original.message_id().clone(),
                serde_json::to_vec(&wire)?
            )
            .and_then(|c| c.validate())
            .is_err()
        );
    }
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize, rostfrei::DomainEvent)]
#[domain(id = "private-actor-event", label = "Private actor event")]
struct PrivateEvent {
    amount: i64,
}
#[derive(rostfrei::AggregateEvents)]
enum PrivateEvents {
    Changed(PrivateEvent),
}
struct PrivateAggregate;
impl Aggregate for PrivateAggregate {
    type State = ();
    type Event = PrivateEvents;
    const BOUNDED_CONTEXT: &'static str = "modular";
    const AGGREGATE_TYPE: &'static str = "private";
    fn initial(_: &StreamId) {}
    fn apply(_: &mut (), _: &PrivateEvents) {}
}
#[derive(serde::Serialize, serde::Deserialize)]
struct PublicEvent {
    amount: i64,
}
impl rostfrei::IntegrationEvent for PublicEvent {
    const EVENT_NAME: &'static str = "public-actor-event";
    const SCHEMA_VERSION: u32 = 1;
}
struct PublicMapper;
impl rostfrei::IntegrationEventMapper<PrivateEvent> for PublicMapper {
    type Output = PublicEvent;
    fn map(&self, event: &CommittedDomainEvent<'_, PrivateEvent>) -> PublicEvent {
        PublicEvent {
            amount: event.event().amount,
        }
    }
}
struct CommandMapper;
impl rostfrei::IntegrationCommandMapper<PublicEvent> for CommandMapper {
    type Command = Change;
    type Error = std::convert::Infallible;
    fn map(&self, event: &PublicEvent) -> Result<Change, Self::Error> {
        Ok(Change {
            amount: event.amount,
            reject: false,
        })
    }
}
#[derive(Default)]
struct Publications(Mutex<Vec<rostfrei::EncodedIntegrationMessage>>);
#[async_trait]
impl rostfrei::IntegrationMessageAdapter for Publications {
    async fn publish(
        &self,
        message: rostfrei::EncodedIntegrationMessage,
    ) -> Result<rostfrei_messaging_core::PublishReceipt, rostfrei::IntegrationEventBusError> {
        self.0.lock().unwrap().push(message);
        Ok(rostfrei_messaging_core::PublishReceipt::new(false))
    }
}
#[tokio::test]
async fn private_public_and_integration_to_command_helpers_preserve_attribution() -> TestResult {
    use rostfrei::IntegrationEventDispatcherExt;
    let op = OperationId::new("private-source")?;
    let stream = StreamId::new(AggregateType::new("private")?, AggregateId::new("one")?);
    let commit = derive_commit_id(&stream, &op);
    let event = RecordedEvent::new(
        stream,
        StreamVersion::new(1),
        derive_event_id(&commit, 0),
        commit,
        op,
        ContentFingerprint::digest("source"),
        "private-actor-event",
        1,
        br#"{"amount":7}"#.to_vec(),
    )?
    .with_correlation_id(CorrelationId::new("flow")?)
    .with_actor(actor("alice"));
    let published = Arc::new(Publications::default());
    let public_bus = rostfrei::IntegrationEventBus::new(context()?, published.clone());
    let mut dispatcher = DomainEventDispatcher::new();
    dispatcher.register_integration_event::<PrivateAggregate, PrivateEvent, _>(
        public_bus.clone(),
        PublicMapper,
    )?;
    dispatcher.dispatch(&event).await?;
    let emitted = published.0.lock().unwrap()[0].clone();
    let envelope = emitted.decode::<PublicEvent>()?;
    assert_eq!(envelope.actor(), event.actor());
    assert_eq!(
        envelope.causation_id().unwrap().as_str(),
        event.event_id().as_str()
    );
    // Public creation uses the existing context helper, with no payload attribution field.
    let public = public_bus.encode(
        rostfrei::CommittedEventContext::new(&event)?,
        PublicEvent { amount: 7 },
    )?;
    assert_eq!(public.decode::<PublicEvent>()?.actor(), event.actor());
    let target_store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = rostfrei::IntegrationEventProcessor::new(
        bus(baseline_processor(target_store.clone(), calls.clone())?)?,
        context()?.durable_name("actor-consumer", 1)?,
        CommandMapper,
    );
    processor.process(&envelope).await?;
    processor.process(&envelope).await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let target = target_store.load(&modular_support::stream()?).await?;
    assert_eq!(target.len(), 1);
    assert_eq!(target[0].actor(), event.actor());
    // Same source-message ID maps to the SAME operation even when attribution is changed.
    let changed = envelope.with_actor(actor("bob"))?;
    let response = processor.process(&changed).await?;
    assert!(
        matches!(response.response().outcome(), CommandResponseOutcome::Rejected(r) if r.code().as_str() == "rostfrei.operation.identity-conflict")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        target_store.load(&modular_support::stream()?).await?,
        target
    );
    Ok(())
}
