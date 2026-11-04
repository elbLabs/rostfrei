//! Maintained private-event evolution example: genuine v1 history, evolved v2 code.
//! The NATS case runs with `scripts/test_nats.py`.
#![allow(clippy::panic_in_result_fn)]

#[path = "../examples/event_evolution/inventory.rs"]
mod inventory;

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use inventory::{
    Condition, ExampleResult, Inventory, InventoryState, RetireBicycle, RetireBicycleHandler,
    RetirementReason, metadata, seed_v1_history, stream,
};
use rostfrei::core::{
    ContentFingerprint, EventBatch, EventCodecErrorKind, EventHistory, EventStore, ExecutionStore,
    NewEvent, OperationId, derive_commit_id, derive_event_id,
};
use rostfrei::fixtures::{Fixture, MessageSeriesEngine};
use rostfrei::nats::{NatsEventStore, NatsEventStoreConfig, provision_event_store};
use rostfrei::tracer::{
    ApplicationBindingError, ApplicationBindings, ExposeTracePayloadsForLocalDevelopment,
    NatsRuntime, NatsRuntimeConfig, OperationMode, TestDefinition, TestReportStatus,
};
use rostfrei::{Aggregate, ApplicationName, CommandExecutor, ExpectedVersion, InMemoryEventStore};
use serde_json::json;

rostfrei::install_macro_support!();
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// An original release reads only v1 and its original field shapes.
fn original_reader(history: &[rostfrei::RecordedEvent]) -> ExampleResult<InventoryState> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Registered {
        bicycle_id: String,
        needs_repair: bool,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Retired {
        bicycle_id: String,
    }
    let mut state = InventoryState::default();
    for event in history {
        if event.schema_version() != 1 {
            return Err("original release cannot read this schema".into());
        }
        match event.event_type() {
            "bicycle-registered" => {
                let old: Registered = serde_json::from_slice(event.payload())?;
                state.active.insert(
                    old.bicycle_id,
                    if old.needs_repair {
                        Condition::MaintenanceRequired
                    } else {
                        Condition::Serviceable
                    },
                );
            }
            "bicycle-retired" => {
                let old: Retired = serde_json::from_slice(event.payload())?;
                state.active.remove(&old.bicycle_id);
                state
                    .retired
                    .insert(old.bicycle_id, RetirementReason::Unrecorded);
            }
            _ => return Err("unknown original event".into()),
        }
    }
    Ok(state)
}

async fn rolling_release<S>(store: S) -> ExampleResult
where
    S: EventStore + EventHistory + ExecutionStore + Clone,
{
    let original = seed_v1_history(&store).await?;
    let compatibility =
        CommandExecutor::with_codecs(store.clone(), Arc::new(inventory::compatibility_codecs()?));
    let lossy = compatibility
        .execute(
            &RetireBicycleHandler,
            metadata("lossy-v1-write")?,
            &RetireBicycle {
                bicycle_id: "bike-2".into(),
                reason: RetirementReason::Damaged,
            },
        )
        .await
        .err()
        .ok_or("v1 must reject a reason it cannot preserve")?;
    assert!(lossy.to_string().contains("EncodingFailed"), "{lossy}");
    assert_eq!(
        store.load(&stream()?).await?,
        original,
        "lossy writes commit nothing"
    );
    assert!(
        store
            .load_completed_outcome_in_context(
                &rostfrei::BoundedContextName::new(Inventory::BOUNDED_CONTEXT)?,
                &OperationId::new("lossy-v1-write")?,
            )
            .await?
            .is_none(),
        "encoding failure cannot leave a successful outcome"
    );
    compatibility
        .execute(
            &RetireBicycleHandler,
            metadata("compatibility-retirement")?,
            &RetireBicycle {
                bicycle_id: "bike-1".into(),
                reason: RetirementReason::Unrecorded,
            },
        )
        .await?;
    let compatible_history = store.load(&stream()?).await?;
    assert_eq!(
        original_reader(&compatible_history)?,
        *compatibility
            .rehydrate::<Inventory>(&stream()?)
            .await?
            .state()
    );

    // All affected readers now understand v2; deploy the new writer.
    let current = CommandExecutor::with_codecs(store.clone(), Arc::new(inventory::codecs()?));
    current
        .execute(
            &RetireBicycleHandler,
            metadata("new-writer-retirement")?,
            &RetireBicycle {
                bicycle_id: "bike-2".into(),
                reason: RetirementReason::Damaged,
            },
        )
        .await?;
    let mixed = store.load(&stream()?).await?;
    assert_eq!(
        mixed.get(..compatible_history.len()),
        Some(compatible_history.as_slice())
    );
    assert_eq!(mixed.last().ok_or("new write")?.schema_version(), 2);
    assert!(
        original_reader(&mixed).is_err(),
        "rollback to original is no longer safe"
    );
    let expected = state(
        &[],
        &[
            ("bike-1", RetirementReason::Unrecorded),
            ("bike-2", RetirementReason::Damaged),
            ("bike-3", RetirementReason::Unrecorded),
        ],
    );
    assert_eq!(
        current.rehydrate::<Inventory>(&stream()?).await?.state(),
        &expected
    );
    assert_eq!(
        compatibility
            .rehydrate::<Inventory>(&stream()?)
            .await?
            .state(),
        &expected,
        "rollback to the dual reader remains safe"
    );
    Ok(())
}

#[tokio::test]
async fn rolling_release_preserves_meaning_and_supports_dual_reader_rollback() -> ExampleResult {
    rolling_release(InMemoryEventStore::new()).await
}

/// Real Test publication through NATS workers, with an owned Test namespace.
#[tokio::test]
async fn compatibility_release_behavior_on_nats() -> ExampleResult {
    let connection = rostfrei::nats::connect(&rostfrei::nats::NatsConnectionConfig::new(
        "compatibility-release",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let context = ApplicationName::new(format!(
        "compatibility-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))?
    .test_bounded_context(Inventory::BOUNDED_CONTEXT)?;
    let mut bindings =
        ApplicationBindings::new().with_event_codecs(Arc::new(inventory::compatibility_codecs()?));
    bindings.register_aggregate::<Inventory>()?;
    bindings.register_command::<RetireBicycle, _>(
        RetireBicycleHandler,
        rostfrei::JsonDomainRejectionMapper::new(
            rostfrei::CommandRejectionClassification::Conflict,
        ),
    )?;
    let config = NatsRuntimeConfig::new(context, &bindings, 1)?
        .with_fresh_completed_outcome_epoch("compatibility-release")?;
    let runtime =
        Arc::new(NatsRuntime::provision(connection.clone(), config, Arc::new(bindings)).await?);
    let fixture: Fixture = serde_json::from_value(json!({
        "schemaVersion": 1, "id": "v1-inventory", "revision": "1",
        "messages": [{
            "kind": "domain-event", "messageId": "old-bike-1", "correlationId": "fixture:v1-inventory",
            "name": "bicycle-registered", "schemaVersion": 1,
            "aggregate": {"type": Inventory::AGGREGATE_TYPE, "id": inventory::INVENTORY_ID},
            "streamVersion": 1, "payload": {"bicycle_id": "bike-1", "needs_repair": false}
        }]
    }))?;
    let tracer = runtime
        .tracer_builder()
        .with_test_transport(runtime.transport())
        .with_test_scenario_reset(runtime.clone())
        .with_default_test_fixture(fixture)
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment))
        .build()?;
    runtime
        .attach_observer(tracer.correlation_observer(OperationMode::Test))
        .await?;
    assert_eq!(
        tracer
            .catalog()
            .behavioral_test
            .as_ref()
            .ok_or("behavior capability")?
            .schema_href,
        "/schemas/behavioral-test-v2"
    );
    let definition = TestDefinition::from_json_value(serde_json::from_str(include_str!(
        "../examples/event_evolution/compatibility-release.json"
    ))?)?;
    let report = tracer.run_inline_test(definition).await;
    runtime.stop_workers().await;
    let report = report?;
    println!(
        "compatibility release evidence: {}",
        serde_json::to_string_pretty(&report)?
    );
    assert_eq!(
        report.status,
        TestReportStatus::Passed,
        "{}",
        serde_json::to_string_pretty(&report)?
    );
    let history = runtime.store().load(&stream()?).await?;
    assert_eq!(
        original_reader(&history)?,
        state(&[], &[("bike-1", RetirementReason::Unrecorded)])
    );
    connection.drain().await?;
    Ok(())
}

fn state(active: &[(&str, Condition)], retired: &[(&str, RetirementReason)]) -> InventoryState {
    let owned = |id: &str| id.to_owned();
    InventoryState {
        active: active
            .iter()
            .map(|(id, c)| (owned(id), *c))
            .collect::<BTreeMap<_, _>>(),
        retired: retired.iter().map(|(id, r)| (owned(id), *r)).collect(),
    }
}

/// v1 history -> current meaning -> v2 write -> mixed reload, on any store.
async fn evolve<S>(store: S) -> ExampleResult
where
    S: EventStore + EventHistory + ExecutionStore + Clone,
{
    let original = seed_v1_history(&store).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), Arc::new(inventory::codecs()?));
    assert_eq!(
        executor.rehydrate::<Inventory>(&stream()?).await?.state(),
        &state(
            &[
                ("bike-1", Condition::Serviceable),
                ("bike-2", Condition::MaintenanceRequired),
            ],
            &[("bike-3", RetirementReason::Unrecorded)],
        ),
        "v1 meaning is preserved, a missing reason stays explicitly unrecorded"
    );
    executor
        .execute(
            &RetireBicycleHandler,
            metadata("retire-bike-2")?,
            &RetireBicycle {
                bicycle_id: "bike-2".to_owned(),
                reason: RetirementReason::Damaged,
            },
        )
        .await?;
    let history = store.load(&stream()?).await?;
    assert_eq!(
        history.get(..original.len()),
        Some(original.as_slice()),
        "reads and new writes never rewrite stored v1 records"
    );
    let written = history.last().ok_or("missing new event")?;
    assert_eq!(
        (written.event_type(), written.schema_version()),
        ("bicycle-retired", 2)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(written.payload())?,
        json!({"bicycle_id": "bike-2", "reason": "damaged"})
    );
    assert_eq!(
        executor.rehydrate::<Inventory>(&stream()?).await?.state(),
        &state(
            &[("bike-1", Condition::Serviceable)],
            &[
                ("bike-2", RetirementReason::Damaged),
                ("bike-3", RetirementReason::Unrecorded),
            ],
        )
    );
    // Current behavior runs against reconstructed state: bike-3 is no longer active.
    let rejected = executor
        .simulate(
            &RetireBicycleHandler,
            metadata("retire-bike-3")?,
            &RetireBicycle {
                bicycle_id: "bike-3".to_owned(),
                reason: RetirementReason::Sold,
            },
        )
        .await?;
    assert!(matches!(
        rejected.decision(),
        rostfrei::CommandDecision::Rejected(inventory::UnknownBicycle { bicycle_id }) if bicycle_id == "bike-3"
    ));
    assert!(
        rejected
            .participants()
            .iter()
            .all(|p| p.events().is_empty())
    );
    Ok(())
}

#[tokio::test]
async fn evolved_code_reconstructs_v1_history_and_reloads_mixed_history_in_memory() -> ExampleResult
{
    evolve(InMemoryEventStore::new()).await
}

#[tokio::test]
async fn evolved_code_reconstructs_v1_history_and_reloads_mixed_history_on_nats() -> ExampleResult {
    let client = rostfrei::nats::connect(&rostfrei::nats::NatsConnectionConfig::new(
        "event-evolution",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?
    .client()
    .clone();
    let js = async_nats::jetstream::new(client);
    let context = ApplicationName::new(format!(
        "evolution-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))?
    .bounded_context(Inventory::BOUNDED_CONTEXT)?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_fresh_split_outcome_epoch("event-evolution")?
        .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
    provision_event_store(&js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(js.clone(), config.clone()).await?);
    let result = evolve(store).await;
    js.delete_stream(config.stream_name()).await?;
    result
}

#[tokio::test]
async fn rolling_release_preserves_meaning_on_nats() -> ExampleResult {
    let connection = rostfrei::nats::connect(&rostfrei::nats::NatsConnectionConfig::new(
        "rolling-evolution",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let js = connection.jetstream();
    let context = ApplicationName::new(format!(
        "rolling-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))?
    .test_bounded_context(Inventory::BOUNDED_CONTEXT)?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_fresh_split_outcome_epoch("rolling-release")?
        .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
    provision_event_store(js, &config).await?;
    let result = rolling_release(Arc::new(
        NatsEventStore::connect(js.clone(), config.clone()).await?,
    ))
    .await;
    js.delete_stream(config.stream_name()).await?;
    connection.drain().await?;
    result
}

#[tokio::test]
async fn historical_fixtures_validate_through_the_shared_registry() -> ExampleResult {
    let codecs = Arc::new(inventory::codecs()?);
    let mut bindings = ApplicationBindings::new().with_event_codecs(codecs.clone());
    bindings.register_aggregate::<Inventory>()?;
    let fixture = |payload: serde_json::Value| -> ExampleResult<Fixture> {
        Ok(serde_json::from_value(json!({
            "schemaVersion": 1,
            "id": "v1-inventory",
            "revision": "1",
            "messages": [{
                "kind": "domain-event",
                "messageId": "v1-bike-1",
                "correlationId": "fixture:v1-inventory",
                "name": "bicycle-registered",
                "schemaVersion": 1,
                "aggregate": {"type": Inventory::AGGREGATE_TYPE, "id": inventory::INVENTORY_ID},
                "streamVersion": 1,
                "payload": payload,
            }],
        }))?)
    };
    let store = InMemoryEventStore::new();
    let engine = MessageSeriesEngine::with_codecs(codecs.clone());
    engine
        .apply(
            &store,
            &fixture(json!({"bicycle_id": "bike-1", "needs_repair": true}))?,
        )
        .await?;
    assert_eq!(
        CommandExecutor::with_codecs(store.clone(), codecs.clone())
            .rehydrate::<Inventory>(&stream()?)
            .await?
            .state(),
        &state(&[("bike-1", Condition::MaintenanceRequired)], &[])
    );
    let empty = InMemoryEventStore::new();
    assert!(
        engine
            .apply(&empty, &fixture(json!({"bicycle_id": "bike-1"}))?)
            .await
            .is_err(),
        "a malformed historical fixture is rejected before any write"
    );
    assert!(empty.load(&stream()?).await?.is_empty());
    assert!(matches!(
        ApplicationBindings::new()
            .with_event_codecs(Arc::new(rostfrei::AggregateCodecs::default()))
            .register_aggregate::<Inventory>(),
        Err(ApplicationBindingError::UnknownAggregateCodec { .. })
    ));
    Ok(())
}

#[tokio::test]
async fn unsupported_history_fails_reconstruction_without_partial_state() -> ExampleResult {
    let store = InMemoryEventStore::new();
    seed_v1_history(&store).await?;
    let operation = OperationId::new("future-history")?;
    let commit = derive_commit_id(&stream()?, &operation);
    store
        .append(
            &stream()?,
            ExpectedVersion::Exact(rostfrei::core::StreamVersion::new(4)),
            EventBatch::new(
                commit.clone(),
                operation,
                ContentFingerprint::digest("future-history"),
                vec![NewEvent::new(
                    derive_event_id(&commit, 0),
                    "bicycle-retired",
                    3,
                    br#"{"bicycle_id":"bike-1"}"#.to_vec(),
                )?],
            )?,
        )
        .await?;
    let error = CommandExecutor::with_codecs(store, Arc::new(inventory::codecs()?))
        .rehydrate::<Inventory>(&stream()?)
        .await
        .err()
        .ok_or("unsupported schema must not reconstruct")?;
    assert!(
        error.to_string().contains(&format!(
            "{:?}",
            EventCodecErrorKind::UnsupportedSchemaVersion
        )),
        "diagnostic names the unsupported schema: {error}"
    );
    Ok(())
}
