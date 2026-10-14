//! #85: desired execution-error classification and durable incoming-trigger isolation.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use async_trait::async_trait;
use release_contract_support::*;
use rostfrei::{CommandBusErrorKind, CommandProcessorErrorKind, IntegrationEventCommandHandler};

async fn fault(
    codec: Option<BadCodec>,
    kind: EventStoreErrorKind,
) -> TestResult<(Fixture, Arc<FaultStore>, Arc<CommandProcessor>, CommandBus)> {
    let fixture = Fixture::new("85-boundary").await?;
    let store = Arc::new(FaultStore {
        inner: InMemoryEventStore::new(),
        kind,
        active: Arc::new(AtomicBool::new(codec.is_none())),
    });
    let calls = Arc::new(AtomicUsize::new(0));
    if matches!(codec, Some(BadCodec::Decode)) {
        local_bus(
            &fixture.context,
            processor(&fixture.context, store.clone(), calls.clone(), None)?,
        )
        .dispatch(request("seed", "bad-target", "create")?)
        .await?;
    }
    let processor = processor(&fixture.context, store.clone(), calls, codec)?;
    let local = local_bus(&fixture.context, processor.clone());
    Ok((fixture, store, processor, local))
}

async fn processor_permanent(codec: Option<BadCodec>) -> TestResult {
    let (fixture, store, processor, local) =
        fault(codec, EventStoreErrorKind::CorruptHistory).await?;
    let encoded = local.encode(request("permanent", "bad-target", "create")?)?;
    let mut codecs = AggregateCodecs::default();
    if let Some(codec) = codec {
        codecs.register::<Target, _>(codec);
    }
    let raw = CommandExecutor::with_codecs(store.clone(), Arc::new(codecs))
        .execute(
            &Handler(Arc::new(AtomicUsize::new(0))),
            CommandExecutionMetadata::new(OperationId::new("permanent")?, encoded.fingerprint())
                .with_bounded_context(fixture.context.name().clone())
                .with_correlation_id(CorrelationId::new("contract-correlation")?)
                .with_causation_id(CausationId::new("contract-causation")?),
            &Act {
                target: "bad-target".to_owned(),
                action: "create".to_owned(),
                item: "I".to_owned(),
            },
        )
        .await;
    match (raw, codec) {
        (Err(CommandExecutionError::Store(e)), None) => {
            assert_eq!(e.kind(), EventStoreErrorKind::CorruptHistory);
        }
        (Err(CommandExecutionError::Codec(e)), Some(BadCodec::Decode)) => {
            assert_eq!(e.kind(), EventCodecErrorKind::MalformedPayload);
        }
        (Err(CommandExecutionError::Codec(e)), Some(BadCodec::Encode)) => {
            assert_eq!(e.kind(), EventCodecErrorKind::EncodingFailed);
        }
        _ => {
            return Err("fault fixture did not exercise the intended typed execution error".into());
        }
    }
    let error = processor.process(&encoded).await.unwrap_err();
    assert!(
        store
            .inner
            .load_transaction_receipt_in_context(
                fixture.context.name(),
                &OperationId::new("permanent")?
            )
            .await?
            .is_none()
    );
    assert!(
        !error.is_retryable(),
        "permanent execution failure must not be retried: {:?}",
        error.kind()
    );
    assert_ne!(error.kind(), CommandProcessorErrorKind::Unavailable);
    assert_ne!(error.kind(), CommandProcessorErrorKind::InvalidMessage);
    assert_ne!(
        error.kind(),
        CommandProcessorErrorKind::InvalidConfiguration
    );
    Ok(())
}
async fn local_permanent(codec: Option<BadCodec>) -> TestResult {
    let (_, _, _, local) = fault(codec, EventStoreErrorKind::CorruptHistory).await?;
    let error = local
        .dispatch(request("permanent", "bad-target", "create")?)
        .await
        .unwrap_err();
    assert_ne!(
        error.kind(),
        CommandBusErrorKind::Unavailable,
        "local boundary lost permanent classification"
    );
    assert_ne!(error.kind(), CommandBusErrorKind::Timeout);
    assert_ne!(
        error.kind(),
        CommandBusErrorKind::Rejected,
        "technical fault is not a domain rejection"
    );
    assert_permanent_bus_kind(error.kind());
    Ok(())
}
macro_rules! boundary_cases {
    ($processor:ident, $local:ident, $codec:expr) => {
        #[tokio::test]
        async fn $processor() -> TestResult {
            processor_permanent($codec).await
        }
        #[tokio::test]
        async fn $local() -> TestResult {
            local_permanent($codec).await
        }
    };
}
boundary_cases!(
    corrupt_history_processor_is_permanent,
    corrupt_history_local_is_permanent,
    None
);
boundary_cases!(
    decode_failure_processor_is_permanent,
    decode_failure_local_is_permanent,
    Some(BadCodec::Decode)
);
boundary_cases!(
    encode_failure_processor_is_permanent,
    encode_failure_local_is_permanent,
    Some(BadCodec::Encode)
);

async fn integration_permanent(codec: Option<BadCodec>) -> TestResult {
    let (fixture, store, _, bus) = fault(codec, EventStoreErrorKind::CorruptHistory).await?;
    let message = trigger(&fixture.context, "bad-target")?;
    let handler = IntegrationEventCommandHandler::<Trigger, _>::new(
        bus,
        fixture.context.durable_name("classification", 1)?,
        RetryDelay::new(Duration::from_millis(100))?,
        Mapper,
    );
    let delivery = MessageDelivery::new_with_transport_context(
        message.address().clone(),
        message.message_id().clone(),
        message.payload().to_vec(),
        CallerMetadata::new(),
        message.correlation_id().cloned(),
        None,
        DeliveryInfo::new(1, 0, 1, 1)?,
    )?;
    let disposition = handler.handle(delivery).await;
    assert_eq!(
        store.inner.load(&stream("bad-target")?).await?.len(),
        usize::from(matches!(codec, Some(BadCodec::Decode)))
    );
    assert!(
        matches!(disposition, DeliveryDisposition::Quarantine(_)),
        "permanent technical failure requires durable incoming-trigger isolation, not {disposition:?}"
    );
    Ok(())
}
#[tokio::test]
async fn corrupt_history_integration_selects_quarantine() -> TestResult {
    integration_permanent(None).await
}
#[tokio::test]
async fn decode_failure_integration_selects_quarantine() -> TestResult {
    integration_permanent(Some(BadCodec::Decode)).await
}
#[tokio::test]
async fn encode_failure_integration_selects_quarantine() -> TestResult {
    integration_permanent(Some(BadCodec::Encode)).await
}

#[tokio::test]
async fn invalid_store_configuration_is_not_a_transient_outage() -> TestResult {
    let (_, _, processor, bus) = fault(None, EventStoreErrorKind::ConfigurationMismatch).await?;
    let error = processor
        .process(&bus.encode(request("configuration", "bad-target", "create")?)?)
        .await
        .unwrap_err();
    assert!(!error.is_retryable());
    Ok(())
}
#[tokio::test]
async fn unavailable_processor_and_local_bus_remain_transient_without_receipt() -> TestResult {
    let (fixture, store, processor, bus) = fault(None, EventStoreErrorKind::Unavailable).await?;
    let request = request("transient", "bad-target", "create")?;
    let encoded = bus.encode(request.clone())?;
    let error = processor.process(&encoded).await.unwrap_err();
    assert_eq!(error.kind(), CommandProcessorErrorKind::Unavailable);
    assert!(error.is_retryable());
    assert_eq!(
        bus.dispatch(request).await.unwrap_err().kind(),
        CommandBusErrorKind::Unavailable
    );
    assert!(
        store
            .inner
            .load_transaction_receipt_in_context(
                fixture.context.name(),
                &OperationId::new("transient")?
            )
            .await?
            .is_none()
    );
    Ok(())
}
#[tokio::test]
async fn business_rejection_and_identity_conflict_are_not_technical_errors() -> TestResult {
    let (fixture, store, _, bus) = fault(None, EventStoreErrorKind::Unavailable).await?;
    assert!(rejected(
        bus.dispatch(request("reject", "missing", "add")?)
            .await?
            .response()
            .outcome()
    ));
    for (kind, expected) in [
        (
            EventStoreErrorKind::IdentityConflict,
            CommandRejectionClassification::Conflict,
        ),
        (
            EventStoreErrorKind::InvalidRequest,
            CommandRejectionClassification::InvalidRequest,
        ),
    ] {
        let source = Arc::new(FaultStore {
            inner: store.inner.clone(),
            kind,
            active: Arc::new(AtomicBool::new(true)),
        });
        let local = local_bus(
            &fixture.context,
            processor(
                &fixture.context,
                source,
                Arc::new(AtomicUsize::new(0)),
                None,
            )?,
        );
        let response = local
            .dispatch(request("compatibility", "bad-target", "create")?)
            .await?;
        let CommandResponseOutcome::Rejected(rejection) = response.response().outcome() else {
            return Err("expected existing framework rejection".into());
        };
        assert_eq!(rejection.classification(), expected);
    }
    Ok(())
}

async fn remote_permanent(codec: Option<BadCodec>) -> TestResult {
    let (fixture, store, processor, _) = fault(codec, EventStoreErrorKind::CorruptHistory).await?;
    let _worker = fixture.command_worker(processor).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let error = remote
        .dispatch(request("remote-poison", "bad-target", "create")?)
        .await
        .unwrap_err();
    assert!(
        store
            .inner
            .load_transaction_receipt_in_context(
                fixture.context.name(),
                &OperationId::new("remote-poison")?
            )
            .await?
            .is_none()
    );
    assert_ne!(
        error.kind(),
        CommandBusErrorKind::Timeout,
        "quarantining a delivered command must notify its caller with a typed technical failure"
    );
    assert_ne!(error.kind(), CommandBusErrorKind::Unavailable);
    assert_ne!(error.kind(), CommandBusErrorKind::Rejected);
    assert_permanent_bus_kind(error.kind());
    Ok(())
}
#[tokio::test]
async fn remote_caller_receives_technical_failure_instead_of_timeout_or_rejection() -> TestResult {
    remote_permanent(None).await
}
#[tokio::test]
async fn remote_decode_failure_is_typed_not_timeout() -> TestResult {
    remote_permanent(Some(BadCodec::Decode)).await
}
#[tokio::test]
async fn remote_encode_failure_is_typed_not_timeout() -> TestResult {
    remote_permanent(Some(BadCodec::Encode)).await
}

fn assert_permanent_bus_kind(kind: CommandBusErrorKind) {
    assert!(
        !matches!(
            kind,
            CommandBusErrorKind::Encoding
                | CommandBusErrorKind::PayloadTooLarge
                | CommandBusErrorKind::InvalidMessage
                | CommandBusErrorKind::Timeout
                | CommandBusErrorKind::Rejected
                | CommandBusErrorKind::Unavailable
                | CommandBusErrorKind::InvalidConfiguration
                | CommandBusErrorKind::InvalidResponse
        ),
        "execution faults need a typed permanent classification, not an unrelated transport/request failure: {kind:?}"
    );
}
struct Observed {
    inner: IntegrationEventCommandHandler<Trigger, Mapper>,
    calls: Arc<AtomicUsize>,
    force_quarantine: bool,
}
#[async_trait]
impl MessageHandler<IntegrationEventAddress> for Observed {
    async fn handle(
        &self,
        delivery: MessageDelivery<IntegrationEventAddress>,
    ) -> DeliveryDisposition {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.force_quarantine {
            return DeliveryDisposition::Quarantine(
                QuarantineReason::new("controlled quarantine sink recovery").unwrap(),
            );
        }
        self.inner.handle(delivery).await
    }
}
async fn integration_worker(
    fixture: &Fixture,
    bus: CommandBus,
    concurrency: usize,
    force_quarantine: bool,
) -> TestResult<(
    Worker,
    async_nats::jetstream::consumer::PullConsumer,
    Arc<AtomicUsize>,
)> {
    let config = ConsumerConfig::new(
        fixture.context.consumer_name("trigger", 1)?,
        fixture.context.durable_name("trigger", 1)?,
        fixture.context.integration_event_address("trigger")?,
        Duration::from_secs(5),
        Duration::from_secs(1),
        concurrency,
        1000,
    )?;
    let provisioned =
        provision_durable_consumer(&fixture.js, fixture.messaging.topology(), &config).await?;
    assert_eq!(
        provisioned.config.max_ack_pending,
        i64::try_from(concurrency)?
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let handler = Arc::new(Observed {
        inner: IntegrationEventCommandHandler::new(
            bus,
            config.durable_name().clone(),
            RetryDelay::new(Duration::from_millis(100))?,
            Mapper,
        ),
        calls: calls.clone(),
        force_quarantine,
    });
    let factory =
        NatsConsumerFactory::new(fixture.js.clone(), fixture.messaging.topology().clone())
            .with_quarantine_publish_timeout(Duration::from_millis(200))?;
    let consumer =
        <NatsConsumerFactory as MessageConsumerFactory<IntegrationEventAddress>>::create(
            &factory,
            config.clone(),
        )?;
    let source = fixture
        .js
        .get_stream(
            fixture
                .messaging
                .topology()
                .integration_event_stream()
                .as_str(),
        )
        .await?;
    let observer = source.get_consumer(config.durable_name().as_str()).await?;
    Ok((
        Worker(tokio::spawn(async move { consumer.run(handler).await })),
        observer,
        calls,
    ))
}
async fn wait_drained(
    consumer: &mut async_nats::jetstream::consumer::PullConsumer,
    store: &InMemoryEventStore,
    target: Option<&str>,
) -> TestResult {
    tokio::time::timeout(WAIT, async {
        loop {
            let info = consumer.info().await?;
            let healthy = match target {
                Some(target) => store.load(&stream(target)?).await?.len() == 1,
                None => true,
            };
            if info.num_pending == 0 && info.num_ack_pending == 0 && healthy {
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn more_poison_triggers_than_ack_window_are_isolated_before_healthy_work() -> TestResult {
    let (fixture, store, _, bus) = fault(None, EventStoreErrorKind::CorruptHistory).await?;
    let (_worker, mut observer, calls) = integration_worker(&fixture, bus, 2, false).await?;
    let mut poison = Vec::new();
    for target in ["bad-0", "bad-1", "bad-2", "healthy"] {
        let message = trigger(&fixture.context, target)?;
        if target.starts_with("bad") {
            poison.push(message.clone());
        }
        fixture
            .publisher()
            .publish_integration_event_with_ack(message, Duration::from_secs(2))
            .await?;
    }
    let drained = wait_drained(&mut observer, &store.inner, Some("healthy")).await;
    let info = observer.info().await?;
    println!(
        "window max_ack_pending=2 poison=3 healthy=1 calls={} delivered={} pending={} ack_pending={}",
        calls.load(Ordering::SeqCst),
        info.delivered.consumer_sequence,
        info.num_pending,
        info.num_ack_pending
    );
    assert!(
        drained.is_ok(),
        "healthy independent work must progress within {WAIT:?}, far below 1000 retry attempts: {drained:?}"
    );
    let records = fixture.quarantine().await?;
    assert_eq!(records.len(), 3);
    for message in poison {
        let record = records
            .iter()
            .find(|r| r.message_id() == message.message_id().as_str())
            .ok_or("missing poison evidence")?;
        assert_eq!(
            base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                record.payload_base64()
            )?,
            message.payload()
        );
        assert_eq!(
            record.attempt(),
            1,
            "permanent faults must not consume their retry budget"
        );
    }
    for target in ["bad-0", "bad-1", "bad-2"] {
        assert!(store.inner.load(&stream(target)?).await?.is_empty());
    }
    assert_eq!(
        fixture
            .messages(
                fixture
                    .messaging
                    .topology()
                    .command_response_stream()
                    .as_str()
            )
            .await?,
        0,
        "integration technical faults fabricated no command outcome"
    );
    Ok(())
}

#[tokio::test]
async fn transient_outage_naks_then_same_trigger_recovers_without_quarantine() -> TestResult {
    let (fixture, store, _, bus) = fault(None, EventStoreErrorKind::Unavailable).await?;
    let (_worker, mut observer, calls) = integration_worker(&fixture, bus, 1, false).await?;
    fixture
        .publisher()
        .publish_integration_event_with_ack(
            trigger(&fixture.context, "bad-recover")?,
            Duration::from_secs(2),
        )
        .await?;
    tokio::time::timeout(WAIT, async {
        while calls.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    assert!(store.inner.load(&stream("bad-recover")?).await?.is_empty());
    let before = observer.info().await?;
    assert_eq!(before.num_ack_pending, 1);
    assert_eq!(before.ack_floor.stream_sequence, 0);
    assert_eq!(
        fixture
            .messages(
                fixture
                    .messaging
                    .topology()
                    .command_response_stream()
                    .as_str()
            )
            .await?,
        0
    );
    store.active.store(false, Ordering::SeqCst);
    wait_drained(&mut observer, &store.inner, Some("bad-recover")).await?;
    assert_eq!(
        fixture
            .messages(fixture.messaging.topology().quarantine_stream().as_str())
            .await?,
        0
    );
    Ok(())
}

async fn sink_recovery(force_quarantine: bool) -> TestResult {
    let (fixture, store, _, bus) = fault(None, EventStoreErrorKind::CorruptHistory).await?;
    fixture
        .js
        .delete_stream(fixture.messaging.topology().quarantine_stream().as_str())
        .await?;
    let (_worker, mut observer, calls) =
        integration_worker(&fixture, bus, 1, force_quarantine).await?;
    let message = trigger(&fixture.context, "bad-sink")?;
    fixture
        .publisher()
        .publish_integration_event_with_ack(message.clone(), Duration::from_secs(2))
        .await?;
    // Redelivery proves the sink failure was exercised, not merely a scheduling snapshot.
    tokio::time::timeout(WAIT, async {
        while calls.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    let before = observer.info().await?;
    assert_eq!(
        before.num_ack_pending, 1,
        "no evidence sink means source stays pending"
    );
    assert_eq!(
        before.ack_floor.stream_sequence, 0,
        "source cannot be TERM'd before durable evidence"
    );
    assert!(store.inner.load(&stream("bad-sink")?).await?.is_empty());
    provision_application_messaging(&fixture.js, &fixture.messaging).await?;
    let drained = wait_drained(&mut observer, &store.inner, None).await;
    assert!(
        drained.is_ok(),
        "source must recover and durably quarantine when sink returns: {drained:?}"
    );
    let records = fixture.quarantine().await?;
    assert_eq!(records.len(), 1);
    assert_eq!(
        records
            .first()
            .ok_or("missing quarantine evidence")?
            .message_id(),
        message.message_id().as_str()
    );
    assert_eq!(
        fixture
            .messages(
                fixture
                    .messaging
                    .topology()
                    .command_response_stream()
                    .as_str()
            )
            .await?,
        0
    );
    Ok(())
}
#[tokio::test]
async fn native_permanent_failure_waits_for_sink_and_recovers() -> TestResult {
    sink_recovery(false).await
}
#[tokio::test]
async fn existing_quarantine_puback_before_term_sink_recovery_control() -> TestResult {
    sink_recovery(true).await
}

rostfrei::install_macro_support!();
