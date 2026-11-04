#![allow(
    clippy::unwrap_used,
    reason = "fixed valid codec fixtures and assertion helpers"
)]

use std::sync::{Arc, Mutex};

use rostfrei::{
    AggregateCodecs, AggregateInstance, CodecFailureKind, CommandExecutionError, EventCodec,
    EventCodecBuildError, EventCodecBuilder, EventCodecError, EventCodecErrorKind, EventId,
    ExecutionFailure, ExecutionRecovery, HistoricalEventCodec, NewEvent, RecordedEvent, StreamId,
    StreamVersion,
};
use serde::{Deserialize, Serialize};

#[path = "ui/event_codec_support.rs"]
mod model;
use model::*;

rostfrei::install_macro_support!();

struct InventoryCodec;

#[rostfrei::event_codec(aggregate = Inventory)]
impl InventoryCodec {
    #[upcast]
    pub const fn added(old: AddedV1) -> Added {
        added_v1(old)
    }

    #[upcast]
    pub fn retired(old: RetiredV1) -> Result<Retired, &'static str> {
        retired_v1(old)
    }

    const fn ordinary_helper() -> &'static str {
        "ordinary Rust methods remain available"
    }
}

fn manual_builder() -> EventCodecBuilder<Inventory> {
    EventCodecBuilder::new()
        .upcast(InventoryCodec::added)
        .try_upcast(InventoryCodec::retired)
}

fn stream() -> StreamId {
    StreamId::new(
        rostfrei::StreamAggregateType::new(<Inventory as rostfrei::Aggregate>::aggregate_type())
            .unwrap(),
        rostfrei::StreamAggregateId::new("warehouse-7").unwrap(),
    )
}

fn recorded(event_type: &str, version: u32, payload: &[u8]) -> RecordedEvent {
    RecordedEvent::new(
        stream(),
        StreamVersion::new(7),
        EventId::new("original-event").unwrap(),
        rostfrei::core::CommitId::new("original-commit").unwrap(),
        rostfrei::OperationId::new("original-operation").unwrap(),
        rostfrei::ContentFingerprint::digest(b"original-request"),
        event_type,
        version,
        payload,
    )
    .unwrap()
    .with_correlation_id(rostfrei::CorrelationId::new("original-correlation").unwrap())
    .with_causation_id(rostfrei::CausationId::new("original-causation").unwrap())
}

#[test]
fn macro_and_manual_composition_read_history_and_write_current_json() {
    let macro_codec: HistoricalEventCodec<Inventory> = InventoryCodec::builder().build().unwrap();
    let manual_codec = manual_builder().build().unwrap();
    let history = [
        recorded("stock-added", 1, br#"{"count":9}"#),
        recorded("stock-retired", 1, br#"{"count":2}"#),
        recorded("stock-added", 2, br#"{"quantity":4}"#),
    ];
    let original = history.clone();
    for codec in [&macro_codec, &manual_codec] {
        let events = history.iter().map(|event| codec.decode(event).unwrap());
        let inventory = AggregateInstance::<Inventory>::rehydrate(stream(), events);
        assert_eq!(inventory.state().available, 11);
        assert!(inventory.uncommitted_events().is_empty());
        assert_eq!(
            codec.decode(&history[1]).unwrap(),
            InventoryEvents::Retired(Retired {
                quantity: 2,
                reason: "unrecorded".to_owned()
            }),
        );
        let id = EventId::new("new-event").unwrap();
        let encoded = codec
            .encode(&Added { quantity: 5 }.into(), id.clone())
            .unwrap();
        assert_eq!(encoded.event_id(), &id);
        assert_eq!(encoded.event_type(), "stock-added");
        assert_eq!(encoded.schema_version(), 2);
        assert_eq!(encoded.payload(), br#"{"quantity":5}"#);
    }
    assert_eq!(history, original);
    assert_eq!(InventoryCodec::added(AddedV1 { count: 3 }).quantity, 3);
    assert_eq!(
        InventoryCodec::ordinary_helper(),
        "ordinary Rust methods remain available"
    );
}

#[test]
fn built_codec_uses_the_existing_registry_and_preserves_original_facts() {
    let old = recorded("stock-added", 1, br#"{"count":9}"#);
    let original = old.clone();
    let mut codecs = AggregateCodecs::default();
    assert_eq!(
        codecs
            .resolve::<Inventory>()
            .unwrap()
            .decode(&old)
            .unwrap_err()
            .kind(),
        EventCodecErrorKind::UnsupportedSchemaVersion,
    );
    let codec = InventoryCodec::builder().build().unwrap();
    codecs.register::<Inventory, _>(codec).unwrap();
    let first = codecs.resolve::<Inventory>().unwrap();
    let second = codecs.resolve::<Inventory>().unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    codecs
        .validate_history(&stream(), std::slice::from_ref(&old))
        .unwrap();
    assert_eq!(first.decode(&old).unwrap(), Added { quantity: 9 }.into());
    assert_eq!(old, original);
    assert!(
        codecs
            .register::<Inventory, _>(manual_builder().build().unwrap())
            .is_err()
    );
}

fn compatibility_encoder(
    event: &InventoryEvents,
    id: EventId,
) -> Result<NewEvent, EventCodecError> {
    let payload = match event {
        InventoryEvents::Added(event) => serde_json::json!({"count": event.quantity}),
        InventoryEvents::Retired(event) if event.reason == "unrecorded" => {
            serde_json::json!({"count": event.quantity})
        }
        InventoryEvents::Retired(_) => {
            return Err(EventCodecError::new(
                EventCodecErrorKind::EncodingFailed,
                "v1 cannot preserve a recorded retirement reason",
            ));
        }
    };
    NewEvent::new(
        id,
        rostfrei::core::Event::event_type(event),
        1,
        serde_json::to_vec(&payload).unwrap(),
    )
    .map_err(|error| EventCodecError::new(EventCodecErrorKind::InvalidEnvelope, error.to_string()))
}

#[test]
fn explicit_compatibility_encoder_reads_both_versions_and_writes_v1_without_loss() {
    for builder in [manual_builder(), InventoryCodec::builder()] {
        let codec = builder.with_encoder(compatibility_encoder).build().unwrap();
        for event in [
            recorded("stock-added", 1, br#"{"count":9}"#),
            recorded("stock-added", 2, br#"{"quantity":9}"#),
        ] {
            assert_eq!(codec.decode(&event).unwrap(), Added { quantity: 9 }.into());
        }
        let id = EventId::new("compatibility-write").unwrap();
        let encoded = codec
            .encode(&Added { quantity: 9 }.into(), id.clone())
            .unwrap();
        assert_eq!(encoded.event_id(), &id);
        assert_eq!(encoded.event_type(), "stock-added");
        assert_eq!(encoded.schema_version(), 1);
        assert_eq!(encoded.payload(), br#"{"count":9}"#);
        let error = codec
            .encode(
                &Retired {
                    quantity: 1,
                    reason: "damaged".to_owned(),
                }
                .into(),
                id,
            )
            .unwrap_err();
        assert_eq!(error.kind(), EventCodecErrorKind::EncodingFailed);
        assert!(error.message().contains("cannot preserve"));
    }
}

#[test]
fn failure_classification_and_context_are_identical_for_macro_and_manual_codecs() {
    let macro_codec = InventoryCodec::builder().build().unwrap();
    let manual_codec = manual_builder().build().unwrap();
    for (event, expected) in [
        (
            recorded("stock-retired", 1, br#"{"count":-1}"#),
            EventCodecErrorKind::ConversionFailed,
        ),
        (
            recorded("stock-retired", 1, br#"{"count":"invalid"}"#),
            EventCodecErrorKind::MalformedPayload,
        ),
        (
            recorded("stock-retired", 2, br#"{"quantity":"invalid"}"#),
            EventCodecErrorKind::MalformedPayload,
        ),
        (
            recorded("stock-retired", 99, b"{}"),
            EventCodecErrorKind::UnsupportedSchemaVersion,
        ),
        (
            recorded("foreign-event", 1, b"{}"),
            EventCodecErrorKind::UnknownEventType,
        ),
    ] {
        let error = macro_codec.decode(&event).unwrap_err();
        assert_eq!(error, manual_codec.decode(&event).unwrap_err());
        assert_eq!(error.kind(), expected);
    }
    let event = recorded("stock-retired", 1, br#"{"count":-1}"#);
    let error = macro_codec.decode(&event).unwrap_err();
    for context in [
        "stock-retired",
        "schema 1",
        "schema 2",
        "negative historical quantity",
        "original-event",
        "warehouse-7",
    ] {
        assert!(
            error.message().contains(context),
            "missing {context:?}: {error}"
        );
    }
    let failure = CommandExecutionError::Codec(error)
        .execution_failure()
        .unwrap();
    assert_eq!(
        failure,
        ExecutionFailure::Codec(CodecFailureKind::ConversionFailed)
    );
    assert_eq!(failure.recovery(), ExecutionRecovery::Blocked);
    let wire = serde_json::to_value(failure).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"source": "codec", "kind": "conversion_failed"})
    );
    assert_eq!(
        serde_json::from_value::<ExecutionFailure>(wire).unwrap(),
        failure
    );
}

#[derive(Deserialize, rostfrei::DomainEvent)]
#[domain(id = "stock-added", label = "Alternate v1 shape")]
struct OtherAddedV1 {
    total: u32,
}

#[derive(Deserialize)]
struct ZeroVersion {
    count: u32,
}

impl rostfrei::DomainEvent for ZeroVersion {
    const LOCAL_ID: &'static str = "stock-added";
    const LABEL: &'static str = "Invalid version";
    const FIELDS: &'static [rostfrei::FieldDescriptor] = &[];
    const SCHEMA_VERSION: u32 = 0;
}

#[derive(Deserialize, rostfrei::DomainEvent)]
#[domain(id = "previous-event-name", label = "Renamed source")]
struct Renamed {
    count: u32,
}

#[test]
fn builder_rejects_duplicate_wire_mappings_even_when_rust_input_types_differ() {
    let result = manual_builder()
        .upcast(|old: OtherAddedV1| Added {
            quantity: old.total,
        })
        .build();
    assert!(matches!(
        result,
        Err(EventCodecBuildError::DuplicateMapping { .. })
    ));
}

#[test]
fn builder_rejects_shadowing_current_json_and_invalid_source_versions() {
    let current = manual_builder().upcast(|event: Added| event).build();
    assert!(matches!(
        current,
        Err(EventCodecBuildError::CurrentSchemaMapping { .. })
    ));
    let invalid = manual_builder()
        .upcast(|old: ZeroVersion| Added {
            quantity: old.count,
        })
        .build();
    assert!(matches!(
        invalid,
        Err(EventCodecBuildError::InvalidSchemaVersion { .. })
    ));
}

#[test]
fn builder_rejects_renamed_ids_because_dispatch_routes_on_the_original_id() {
    let result = EventCodecBuilder::<Inventory>::new()
        .upcast(|old: Renamed| Added {
            quantity: old.count,
        })
        .build();
    assert!(matches!(
        result,
        Err(EventCodecBuildError::RenamedEvent { .. })
    ));
}

struct DuplicateCodec;

#[rostfrei::event_codec(aggregate = Inventory)]
impl DuplicateCodec {
    #[upcast]
    const fn first(old: AddedV1) -> Added {
        added_v1(old)
    }
    #[upcast]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "upcasts consume their historical input"
    )]
    const fn second(old: OtherAddedV1) -> Added {
        Added {
            quantity: old.total,
        }
    }
}

#[test]
fn macro_runs_the_same_build_validation_as_manual_composition() {
    assert!(matches!(
        DuplicateCodec::builder().build(),
        Err(EventCodecBuildError::DuplicateMapping { .. })
    ));
}

#[derive(Deserialize, Serialize)]
struct ZeroCurrent;

impl rostfrei::DomainEvent for ZeroCurrent {
    const LOCAL_ID: &'static str = "stock-added";
    const LABEL: &'static str = "Invalid current schema";
    const FIELDS: &'static [rostfrei::FieldDescriptor] = &[];
    const SCHEMA_VERSION: u32 = 0;
}

#[derive(rostfrei::AggregateEvents)]
enum InvalidEvents {
    Added(ZeroCurrent),
}

struct InvalidInventory;

impl rostfrei::Aggregate for InvalidInventory {
    type State = ();
    type Event = InvalidEvents;
    const BOUNDED_CONTEXT: &'static str = "warehouse";
    const AGGREGATE_TYPE: &'static str = "invalid-inventory";
    fn initial(_: &StreamId) {}
    fn apply((): &mut (), _: &InvalidEvents) {}
}

#[test]
fn builder_rejects_zero_target_schema_even_for_hand_written_domain_metadata() {
    assert!(matches!(
        EventCodecBuilder::<InvalidInventory>::new()
            .upcast(|_: AddedV1| ZeroCurrent)
            .build(),
        Err(EventCodecBuildError::InvalidSchemaVersion { .. })
    ));
}

#[test]
fn custom_encoder_must_preserve_the_supplied_event_identity() {
    let codec = manual_builder()
        .with_encoder(|event, _| {
            compatibility_encoder(event, EventId::new("replacement-identity").unwrap())
        })
        .build()
        .unwrap();
    let error = codec
        .encode(
            &Added { quantity: 1 }.into(),
            EventId::new("original-identity").unwrap(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), EventCodecErrorKind::InvalidEnvelope);
}

#[derive(Default)]
struct RetiredHandler(Mutex<Vec<(RecordedEvent, Retired)>>);

#[async_trait::async_trait]
impl rostfrei::DomainEventHandler<Retired> for RetiredHandler {
    async fn handle(
        &self,
        event: &rostfrei::CommittedDomainEvent<'_, Retired>,
    ) -> Result<(), rostfrei::DomainEventHandlerError> {
        self.0
            .lock()
            .unwrap()
            .push((event.recorded().clone(), event.event().clone()));
        Ok(())
    }
}

#[tokio::test]
async fn dispatcher_preserves_original_record_and_blocks_failed_conversions() {
    let mut codecs = AggregateCodecs::default();
    codecs
        .register::<Inventory, _>(InventoryCodec::builder().build().unwrap())
        .unwrap();
    let mut dispatcher = rostfrei::DomainEventDispatcher::with_codecs(Arc::new(codecs));
    let handler = Arc::new(RetiredHandler::default());
    dispatcher
        .register::<Inventory, Retired, _>("stock-retired", handler.clone())
        .unwrap();

    let invalid = recorded("stock-retired", 1, br#"{"count":-1}"#);
    let error = dispatcher.dispatch(&invalid).await.unwrap_err();
    assert_eq!(
        error.kind(),
        rostfrei::DomainEventHandlerErrorKind::InvalidCommittedEvent
    );
    assert!(error.message().contains("ConversionFailed"));
    assert!(handler.0.lock().unwrap().is_empty());

    let original = recorded("stock-retired", 1, br#"{"count":2}"#);
    assert_eq!(
        dispatcher.dispatch(&original).await.unwrap(),
        rostfrei::DomainEventDispatchOutcome::Handled
    );
    assert_eq!(
        *handler.0.lock().unwrap(),
        vec![(
            original,
            Retired {
                quantity: 2,
                reason: "unrecorded".to_owned()
            }
        )]
    );
}
