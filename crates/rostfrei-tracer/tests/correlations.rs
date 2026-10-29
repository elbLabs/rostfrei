#![cfg(feature = "http")]
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt as _;
use rostfrei_core::{
    Aggregate, AggregateId, AggregateType, ContentFingerprint, CorrelationChain,
    CorrelationHistory, CorrelationRead, CorrelationRef, CorrelationSnapshot,
    CorrelationSourceKind, CorrelationSourceSnapshot, DomainEventRef, EventBatch, EventHistory,
    EventStore, EventStoreError, EventStoreErrorKind, ExpectedVersion, InMemoryEventStore,
    IntegrationEventRef, NewEvent, OperationId, RecordedEvent, StreamId, derive_commit_id,
    derive_event_id,
};
use rostfrei_messaging_core::{CausationId, CorrelationId, MessageTimestamp};
use rostfrei_registry::DomainRegistry;
use rostfrei_tracer::{
    ExposeTracePayloadsForLocalDevelopment, Tracer, TracerBuilder,
    http::{self, HttpConfig},
};
use serde_json::{Value, json};
use tower::ServiceExt as _;

const CONTROL: &str = "control";
const DISPATCH: &str = "dispatch";
const INSPECTION: &str = "inspection";
const ORDER: &str = "/aggregates/ordering%2Forder/O1/correlations";
const CUSTOMER: &str = "/aggregates/ordering%2Fcustomer/C7/correlations";

macro_rules! aggregate {
    ($name:ident, $aggregate_type:literal) => {
        struct $name;
        impl Aggregate for $name {
            type State = ();
            type Event = ();
            const BOUNDED_CONTEXT: &'static str = "ordering";
            const AGGREGATE_TYPE: &'static str = $aggregate_type;
            fn initial(_: &StreamId) {}
            fn apply((): &mut (), (): &()) {}
        }
    };
}

aggregate!(Order, "ordering/order");
aggregate!(Customer, "ordering/customer");
aggregate!(Shipment, "ordering/shipment");

#[derive(Default)]
struct CountingHistory {
    store: InMemoryEventStore,
    loads: Mutex<Vec<StreamId>>,
}

impl CountingHistory {
    fn loads(&self) -> Vec<StreamId> {
        self.loads.lock().unwrap().clone()
    }

    async fn append(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        correlation: &str,
        causation: Option<&str>,
        payload: Value,
    ) -> RecordedEvent {
        let stream = stream(aggregate_type, aggregate_id);
        let prior = self.store.load(&stream).await.unwrap();
        let operation =
            OperationId::new(format!("{aggregate_type}-{aggregate_id}-{}", prior.len())).unwrap();
        let commit = derive_commit_id(&stream, &operation);
        let mut batch = EventBatch::new(
            commit.clone(),
            operation,
            ContentFingerprint::digest(payload.to_string()),
            vec![
                NewEvent::new(
                    derive_event_id(&commit, 0),
                    "changed",
                    1,
                    serde_json::to_vec(&payload).unwrap(),
                )
                .unwrap(),
            ],
        )
        .unwrap()
        .with_correlation_id(CorrelationId::new(correlation).unwrap());
        if let Some(causation) = causation {
            batch = batch.with_causation_id(CausationId::new(causation).unwrap());
        }
        let expected = if prior.is_empty() {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(rostfrei_core::StreamVersion::new(
                u64::try_from(prior.len()).unwrap(),
            ))
        };
        self.store
            .append(&stream, expected, batch)
            .await
            .unwrap()
            .into_events()
            .remove(0)
    }
}

#[async_trait]
impl EventHistory for CountingHistory {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.loads.lock().unwrap().push(stream_id.clone());
        self.store.load(stream_id).await
    }
}

// The fake supplies already-retained evidence and a captured source snapshot.
// Broker filtering, cutoff capture, and expiry are exercised by the native reader tests.
#[derive(Default)]
struct FakeHistory {
    state: Mutex<FakeReadState>,
}

#[derive(Default)]
struct FakeReadState {
    chains: HashMap<String, CorrelationChain>,
    sources: Vec<CorrelationSourceSnapshot>,
    calls: Vec<Vec<String>>,
    failure: Option<EventStoreError>,
}

fn empty_chain(correlation_id: &str) -> CorrelationChain {
    CorrelationChain {
        correlation_id: correlation_id.to_owned(),
        refs: Vec::new(),
        truncated: false,
    }
}

impl FakeHistory {
    fn calls(&self) -> Vec<Vec<String>> {
        self.state.lock().unwrap().calls.clone()
    }

    fn push(&self, correlation: &str, reference: CorrelationRef) {
        self.state
            .lock()
            .unwrap()
            .chains
            .entry(correlation.to_owned())
            .or_insert_with(|| empty_chain(correlation))
            .refs
            .push(reference);
    }

    fn truncate(&self, correlation: &str) {
        self.state
            .lock()
            .unwrap()
            .chains
            .entry(correlation.to_owned())
            .or_insert_with(|| empty_chain(correlation))
            .truncated = true;
    }
}

#[async_trait]
impl CorrelationHistory for FakeHistory {
    async fn load_correlations(
        &self,
        ids: &[CorrelationId],
    ) -> Result<CorrelationRead, EventStoreError> {
        let mut state = self.state.lock().unwrap();
        state
            .calls
            .push(ids.iter().map(|id| id.as_str().to_owned()).collect());
        if let Some(error) = &state.failure {
            return Err(error.clone());
        }
        Ok(CorrelationRead {
            chains: ids
                .iter()
                .map(|id| {
                    state
                        .chains
                        .get(id.as_str())
                        .cloned()
                        .unwrap_or_else(|| empty_chain(id.as_str()))
                })
                .collect(),
            snapshot: CorrelationSnapshot {
                sources: state.sources.clone(),
            },
        })
    }
}

fn stream(aggregate_type: &str, aggregate_id: &str) -> StreamId {
    StreamId::new(
        AggregateType::new(aggregate_type).unwrap(),
        AggregateId::new(aggregate_id).unwrap(),
    )
}

fn at(millis: u64) -> MessageTimestamp {
    MessageTimestamp::from_unix_milliseconds(millis).unwrap()
}

fn domain(context: &str, event: &RecordedEvent, millis: u64) -> CorrelationRef {
    CorrelationRef::Domain(DomainEventRef::from_recorded(context, event, at(millis)))
}

fn integration(cause: &RecordedEvent, message_id: &str, millis: u64) -> CorrelationRef {
    CorrelationRef::Integration(IntegrationEventRef {
        context: "ordering".to_owned(),
        subject: "shop.integration.ordering.order-placed".to_owned(),
        message_id: message_id.to_owned(),
        event_type: "order-placed".to_owned(),
        schema_version: 1,
        causation_id: Some(cause.event_id().as_str().to_owned()),
        occurred_at: at(millis),
    })
}

struct Scenario {
    ordering: Arc<CountingHistory>,
    billing: Arc<CountingHistory>,
    history: Arc<FakeHistory>,
    events: BTreeMap<&'static str, RecordedEvent>,
}

/// R1: order O1 placed -> integration event -> billing invoice I1 and shipment S1.
/// R2: order O1 amended, billing invoice I2. R3: customer C7 updated, order O1 noted.
async fn scenario() -> Scenario {
    let ordering = Arc::new(CountingHistory::default());
    let billing = Arc::new(CountingHistory::default());
    let history = Arc::new(FakeHistory::default());
    let mut events = BTreeMap::new();
    let placed = ordering
        .append("ordering/order", "O1", "R1", None, json!({"placed": true}))
        .await;
    let invoiced = billing
        .append(
            "billing/invoice",
            "I1",
            "R1",
            Some("message-1"),
            json!({"amount": 10}),
        )
        .await;
    let shipped = ordering
        .append(
            "ordering/shipment",
            "S1",
            "R1",
            Some("message-1"),
            json!({"shipped": true}),
        )
        .await;
    let amended = ordering
        .append("ordering/order", "O1", "R2", None, json!({"amended": true}))
        .await;
    let reinvoiced = billing
        .append("billing/invoice", "I2", "R2", None, json!({"amount": 12}))
        .await;
    let updated = ordering
        .append("ordering/customer", "C7", "R3", None, json!({"vip": true}))
        .await;
    let noted = ordering
        .append("ordering/order", "O1", "R3", None, json!({"noted": true}))
        .await;
    history.push("R1", domain("ordering", &placed, 1));
    history.push("R1", integration(&placed, "message-1", 2));
    history.push("R1", domain("billing", &invoiced, 3));
    history.push("R1", domain("ordering", &shipped, 4));
    history.push("R2", domain("ordering", &amended, 5));
    history.push("R2", domain("billing", &reinvoiced, 6));
    history.push("R3", domain("ordering", &updated, 7));
    history.push("R3", domain("ordering", &noted, 8));
    history.state.lock().unwrap().sources = vec![
        CorrelationSourceSnapshot {
            context: Some("billing".to_owned()),
            kind: CorrelationSourceKind::Domain,
            stream: "SHOP__BILLING_DOMAIN_EVENTS".to_owned(),
            cutoff_sequence: "21".to_owned(),
        },
        CorrelationSourceSnapshot {
            context: Some("ordering".to_owned()),
            kind: CorrelationSourceKind::Domain,
            stream: "SHOP__ORDERING_DOMAIN_EVENTS".to_owned(),
            cutoff_sequence: "18".to_owned(),
        },
        CorrelationSourceSnapshot {
            context: None,
            kind: CorrelationSourceKind::Integration,
            stream: "SHOP__INTEGRATION_EVENTS".to_owned(),
            cutoff_sequence: "9007199254740993".to_owned(),
        },
    ];
    events.insert("placed", placed);
    events.insert("invoiced", invoiced);
    events.insert("amended", amended);
    events.insert("updated", updated);
    events.insert("noted", noted);
    Scenario {
        ordering,
        billing,
        history,
        events,
    }
}

fn domain_model() -> Value {
    json!({
        "boundedContexts": [{"id": "ordering", "label": "Ordering"}],
        "aggregates": [
            {"id": {"context": "ordering", "local": "order"}, "label": "Order", "anchor": true},
            {"id": {"context": "ordering", "local": "customer"}, "label": "Customer", "anchor": true},
            {"id": {"context": "ordering", "local": "shipment"}, "label": "Shipment"},
        ],
    })
}

fn builder(scenario: &Scenario, expose: bool) -> TracerBuilder {
    let mut registry = DomainRegistry::new();
    registry.register_aggregate::<Order>().unwrap();
    registry.register_aggregate::<Customer>().unwrap();
    registry.register_aggregate::<Shipment>().unwrap();
    let mut builder = TracerBuilder::new(Arc::new(InMemoryEventStore::new()), registry)
        .with_domain_model(domain_model())
        .with_correlation_history(scenario.history.clone())
        .with_production_history("ordering", scenario.ordering.clone())
        .with_production_history("billing", scenario.billing.clone());
    if expose {
        builder =
            builder.with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    }
    builder
}

fn tracer(scenario: &Scenario, expose: bool) -> Tracer {
    builder(scenario, expose).build().unwrap()
}

fn router(tracer: Tracer) -> Router {
    http::router(
        tracer,
        HttpConfig::new(CONTROL)
            .unwrap()
            .with_dispatch_token(DISPATCH)
            .unwrap()
            .with_inspection_token(INSPECTION)
            .unwrap(),
    )
}

async fn get(app: &Router, path: &str, token: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder().uri(path);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn expected_snapshot() -> Value {
    json!({"sources": [
        {"context": "billing", "kind": "domain", "stream": "SHOP__BILLING_DOMAIN_EVENTS",
            "cutoff_sequence": "21"},
        {"context": "ordering", "kind": "domain", "stream": "SHOP__ORDERING_DOMAIN_EVENTS",
            "cutoff_sequence": "18"},
        {"kind": "integration", "stream": "SHOP__INTEGRATION_EVENTS",
            "cutoff_sequence": "9007199254740993"},
    ]})
}

fn assert_snapshot(body: &Value) {
    assert_eq!(body["snapshot"], expected_snapshot());
    for obsolete in ["lag", "sources", "health"] {
        assert!(body.get(obsolete).is_none(), "{obsolete}: {body}");
    }
}

fn event_ids(correlation: &Value) -> Vec<&str> {
    correlation["refs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reference| {
            reference["event_id"]
                .as_str()
                .or_else(|| reference["message_id"].as_str())
                .unwrap()
        })
        .collect()
}

fn correlation_ids(view: &Value) -> Vec<&str> {
    view["correlations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|correlation| correlation["correlation_id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn anchor_view_returns_anchor_payloads_without_loading_foreign_streams() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));

    let (status, body) = get(&app, ORDER, Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["anchor"],
        json!({"aggregate_type": "ordering/order", "aggregate_id": "O1"})
    );
    assert_snapshot(&body);
    assert!(body.get("truncated").is_none());
    assert_eq!(correlation_ids(&body), ["R1", "R2", "R3"]);
    assert_eq!(scenario.history.calls(), [vec!["R1", "R2", "R3"]]);
    let correlations = body["correlations"].as_array().unwrap();
    let first = &correlations[0];
    assert_eq!(first["truncated"], false);
    let placed = &scenario.events["placed"];
    assert_eq!(
        first["refs"][0],
        json!({
            "kind": "domain",
            "context": "ordering",
            "aggregate_type": "ordering/order",
            "aggregate_id": "O1",
            "stream_version": 1,
            "event_id": placed.event_id().as_str(),
            "event_type": "changed",
            "occurred_at": 1,
            "payload": {"placed": true},
        })
    );
    assert_eq!(
        first["refs"][1],
        json!({
            "kind": "integration",
            "context": "ordering",
            "subject": "shop.integration.ordering.order-placed",
            "message_id": "message-1",
            "event_type": "order-placed",
            "schema_version": 1,
            "causation_id": placed.event_id().as_str(),
            "occurred_at": 2,
        })
    );
    assert_eq!(first["refs"][2]["context"], "billing");
    assert_eq!(first["refs"][2]["causation_id"], "message-1");
    assert!(first["refs"][2].get("payload").is_none());
    assert!(first["refs"][3].get("payload").is_none());
    assert_eq!(
        event_ids(&correlations[2])[1],
        scenario.events["noted"].event_id().as_str()
    );
    assert!(correlations[2]["refs"][0].get("payload").is_none());
    let order = stream("ordering/order", "O1");
    assert_eq!(scenario.ordering.loads(), std::slice::from_ref(&order));
    assert!(scenario.billing.loads().is_empty());

    let (status, body) = get(
        &app,
        &format!("{ORDER}?include=context:billing"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_snapshot(&body);
    assert_eq!(
        body["correlations"][0]["refs"][2]["payload"],
        json!({"amount": 10})
    );
    assert_eq!(
        body["correlations"][1]["refs"][1]["payload"],
        json!({"amount": 12})
    );
    assert!(body["correlations"][0]["refs"][3].get("payload").is_none());
    assert_eq!(scenario.billing.loads().len(), 2);
    assert_eq!(scenario.ordering.loads(), [order.clone(), order]);

    let (status, body) = get(
        &app,
        &format!("{ORDER}?include=context%3Aordering"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["correlations"][0]["refs"][3]["payload"],
        json!({"shipped": true})
    );
    assert_eq!(
        body["correlations"][2]["refs"][0]["payload"],
        json!({"vip": true})
    );
    assert_eq!(scenario.history.calls(), vec![vec!["R1", "R2", "R3"]; 3]);
}

#[tokio::test]
async fn anchor_matching_loads_payloads_for_refs_appended_after_the_initial_history_read() {
    struct AppendingHistory {
        ordering: Arc<CountingHistory>,
        retained: Arc<FakeHistory>,
        appended: Mutex<Option<RecordedEvent>>,
    }

    #[async_trait]
    impl CorrelationHistory for AppendingHistory {
        async fn load_correlations(
            &self,
            ids: &[CorrelationId],
        ) -> Result<CorrelationRead, EventStoreError> {
            assert_eq!(
                self.ordering.loads(),
                [stream("ordering/order", "O1")],
                "the initial anchor snapshot must already have been loaded"
            );
            let event = self
                .ordering
                .append(
                    "ordering/order",
                    "O1",
                    "R1",
                    None,
                    json!({"appended_during_read": true}),
                )
                .await;
            self.retained.push("R1", domain("ordering", &event, 9));
            *self.appended.lock().unwrap() = Some(event);
            self.retained.load_correlations(ids).await
        }
    }

    let scenario = scenario().await;
    let reader = Arc::new(AppendingHistory {
        ordering: scenario.ordering.clone(),
        retained: scenario.history.clone(),
        appended: Mutex::new(None),
    });
    let app = router(
        builder(&scenario, true)
            .with_correlation_history(reader.clone())
            .build()
            .unwrap(),
    );
    let (status, body) = get(
        &app,
        &format!("{ORDER}?match=ordering/changed.appended_during_read=true"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_snapshot(&body);
    assert_eq!(scenario.history.calls(), [vec!["R1", "R2", "R3"]]);
    assert!(scenario.billing.loads().is_empty());
    let appended = reader.appended.lock().unwrap().clone().unwrap();
    assert_eq!(appended.stream_version().value(), 4);
    assert_eq!(
        correlation_ids(&body),
        ["R1"],
        "the later retained anchor ref must match its own payload without include: {body}"
    );
    let chain = &body["correlations"][0];
    assert_eq!(chain["truncated"], false);
    assert_eq!(chain["refs"].as_array().unwrap().len(), 5);
    assert_eq!(
        chain["refs"][0]["event_id"],
        scenario.events["placed"].event_id().as_str()
    );
    assert_eq!(chain["refs"][0]["payload"], json!({"placed": true}));
    assert_eq!(chain["refs"][4]["event_id"], appended.event_id().as_str());
    assert_eq!(chain["refs"][4]["stream_version"], 4);
    assert_eq!(
        chain["refs"][4]["payload"],
        json!({"appended_during_read": true})
    );
    assert_eq!(
        scenario.ordering.loads(),
        [
            stream("ordering/order", "O1"),
            stream("ordering/order", "O1")
        ],
        "only one anchor reload is needed for the later retained refs"
    );
}

#[tokio::test]
async fn anchor_reload_missing_a_retained_event_fails_instead_of_reporting_no_match() {
    struct FrozenHistory {
        events: Vec<RecordedEvent>,
        loads: Mutex<Vec<StreamId>>,
    }

    #[async_trait]
    impl EventHistory for FrozenHistory {
        async fn load(&self, stream: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
            self.loads.lock().unwrap().push(stream.clone());
            Ok(self.events.clone())
        }
    }

    let scenario = scenario().await;
    let anchor = stream("ordering/order", "O1");
    let stale = Arc::new(FrozenHistory {
        events: scenario.ordering.store.load(&anchor).await.unwrap(),
        loads: Mutex::new(Vec::new()),
    });
    let later = scenario
        .ordering
        .append("ordering/order", "O1", "R1", None, json!({"later": true}))
        .await;
    scenario.history.push("R1", domain("ordering", &later, 9));
    let app = router(
        builder(&scenario, true)
            .with_production_history("ordering", stale.clone())
            .build()
            .unwrap(),
    );
    let (status, body) = get(
        &app,
        &format!("{ORDER}?match=ordering/changed.later=true"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["code"], "correlation-read-failed");
    assert!(body.get("correlations").is_none());
    assert_eq!(*stale.loads.lock().unwrap(), [anchor.clone(), anchor]);
    assert_eq!(scenario.history.calls(), [vec!["R1", "R2", "R3"]]);
}

#[tokio::test]
async fn direct_inspection_returns_retained_references_and_one_source_snapshot() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));
    let (status, body) = get(&app, "/correlations/R1/events", Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["correlation_id"], "R1");
    assert_eq!(body["truncated"], false);
    assert_snapshot(&body);
    assert_eq!(body["refs"].as_array().unwrap().len(), 4);
    assert_eq!(
        event_ids(&body)[0],
        scenario.events["placed"].event_id().as_str()
    );
    assert_eq!(body["refs"][1]["kind"], "integration");
    assert_eq!(body["refs"][1]["message_id"], "message-1");
    assert!(body["refs"][1].get("available").is_none());
    assert!(body["refs"][1].get("sequence").is_none());
    for reference in body["refs"].as_array().unwrap() {
        assert!(reference.get("payload").is_none(), "{reference}");
    }
    assert_eq!(scenario.history.calls(), [vec!["R1"]]);
    assert!(scenario.ordering.loads().is_empty());
    assert!(scenario.billing.loads().is_empty());
}

#[tokio::test]
async fn service_facades_use_one_batched_retained_read_per_query() {
    let scenario = scenario().await;
    let tracer = tracer(&scenario, true);
    let anchor = tracer
        .anchor_correlations("ordering/order", "O1", &BTreeSet::new(), &[])
        .await
        .unwrap();
    let anchor = serde_json::to_value(anchor).unwrap();
    assert_eq!(correlation_ids(&anchor), ["R1", "R2", "R3"]);
    assert_snapshot(&anchor);
    let direct = tracer.retained_correlation("R1").await.unwrap();
    let direct = serde_json::to_value(direct).unwrap();
    assert_eq!(event_ids(&direct), event_ids(&anchor["correlations"][0]));
    assert_snapshot(&direct);
    assert_eq!(
        scenario.history.calls(),
        [vec!["R1", "R2", "R3"], vec!["R1"]]
    );
}

#[tokio::test]
async fn expired_integration_nodes_are_absent_and_dangling_causation_is_preserved() {
    let scenario = scenario().await;
    // Represent a subsequent retained read after the source has expired this message.
    scenario
        .history
        .state
        .lock()
        .unwrap()
        .chains
        .get_mut("R1")
        .unwrap()
        .refs
        .retain(|reference| !matches!(reference, CorrelationRef::Integration(_)));
    let app = router(tracer(&scenario, true));
    for path in [ORDER, "/correlations/R1/events"] {
        let (status, body) = get(&app, path, Some(INSPECTION)).await;
        assert_eq!(status, StatusCode::OK);
        assert_snapshot(&body);
        let chain = if path == ORDER {
            &body["correlations"][0]
        } else {
            &body
        };
        assert_eq!(chain["truncated"], false, "expiry is not read truncation");
        assert_eq!(chain["refs"].as_array().unwrap().len(), 3);
        assert!(!event_ids(chain).contains(&"message-1"));
        assert_eq!(chain["refs"][1]["causation_id"], "message-1");
        assert_eq!(chain["refs"][2]["causation_id"], "message-1");
    }
}

#[tokio::test]
async fn a_complete_empty_retained_query_is_not_found_but_a_truncated_one_is_not() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));
    let (status, body) = get(&app, "/correlations/unknown/events", Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not-found");

    scenario.history.truncate("unknown");
    let (status, body) = get(&app, "/correlations/unknown/events", Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["correlation_id"], "unknown");
    assert_eq!(body["refs"], json!([]));
    assert_eq!(body["truncated"], true);
    assert_snapshot(&body);
    assert_eq!(scenario.history.calls(), [vec!["unknown"], vec!["unknown"]]);
}

#[tokio::test]
async fn source_failure_fails_the_entire_request_instead_of_returning_partial_evidence() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));
    for (kind, message) in [
        (
            EventStoreErrorKind::Unavailable,
            "ordering domain source is unreadable",
        ),
        (
            EventStoreErrorKind::Unavailable,
            "shared integration source is unreadable",
        ),
        (
            EventStoreErrorKind::CorruptHistory,
            "invalid retained envelope",
        ),
    ] {
        scenario.history.state.lock().unwrap().failure = Some(EventStoreError::new(kind, message));
        for path in [
            ORDER,
            "/correlations/R1/events",
            "/correlations/unknown/events",
        ] {
            let (status, body) = get(&app, path, Some(INSPECTION)).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{path}: {body}");
            assert_eq!(body["code"], "correlation-read-failed");
            assert!(body.get("refs").is_none());
            assert!(body.get("correlations").is_none());
            assert!(body.get("snapshot").is_none());
        }
    }
}

#[tokio::test]
async fn match_keeps_correlations_whose_loaded_payloads_carry_the_value() {
    let scenario = scenario().await;
    let noted = scenario
        .ordering
        .append(
            "ordering/order",
            "O1",
            "R4",
            None,
            json!({"customer": {"id": "C7", "tags": ["vip"]}, "lines": 2, "optional": null}),
        )
        .await;
    let invoiced = scenario
        .billing
        .append("billing/invoice", "I3", "R4", None, json!({"amount": 10}))
        .await;
    scenario.history.push("R4", domain("ordering", &noted, 9));
    scenario
        .history
        .push("R4", domain("billing", &invoiced, 10));
    let app = router(tracer(&scenario, true));

    let (status, body) = get(&app, ORDER, Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("matched").is_none());
    assert_eq!(correlation_ids(&body), ["R1", "R2", "R3", "R4"]);

    let (status, body) = get(
        &app,
        &format!("{ORDER}?include=context:billing&match=billing/changed.amount=10"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(correlation_ids(&body), ["R1", "R4"]);
    assert_eq!(
        body["matched"],
        json!([{"context": "billing", "event_type": "changed", "field": "amount", "value": "10"}])
    );
    assert_snapshot(&body);
    assert_eq!(body["correlations"][0]["refs"].as_array().unwrap().len(), 4);

    for (query, expected) in [
        (
            "include=context:billing&match=billing/changed.amount=12.0",
            vec!["R2"],
        ),
        (
            "include=context:billing&match=billing/changed.amount=1e1",
            vec!["R1", "R4"],
        ),
        ("match=ordering/changed.customer.id=C7", vec!["R4"]),
        ("match=ordering/changed.customer.tags.0=vip", vec!["R4"]),
        (
            "include=context:billing&match=billing/changed.amount=10&match=ordering/changed.placed=true",
            vec!["R1"],
        ),
        (
            "include=context:billing&match=billing/changed.amount=12&match=ordering/changed.placed=true",
            vec![],
        ),
        // Other streams in the anchor context still require an explicit include.
        ("match=ordering/changed.vip=true", vec![]),
        (
            "include=context:ordering&match=ordering/changed.vip=true",
            vec!["R3"],
        ),
        ("match=ordering/changed.lines=2", vec!["R4"]),
        ("match=ordering/changed.lines=two", vec![]),
        ("match=ordering/changed.customer=C7", vec![]),
        ("match=ordering/changed.customer.tags=vip", vec![]),
        ("match=ordering/changed.optional=null", vec![]),
        ("match=ordering/changed.missing=null", vec![]),
        ("match=ordering/noted.placed=true", vec![]),
        ("match=ordering/order-placed.placed=true", vec![]),
    ] {
        let (status, body) = get(&app, &format!("{ORDER}?{query}"), Some(INSPECTION)).await;
        assert_eq!(status, StatusCode::OK, "{query}: {body}");
        assert_eq!(correlation_ids(&body), expected, "{query}");
        assert_snapshot(&body);
        assert_eq!(
            body["matched"].as_array().unwrap().len(),
            query.matches("match=").count()
        );
    }
}

#[tokio::test]
async fn match_values_are_url_decoded_and_compared_as_strings() {
    let scenario = scenario().await;
    let noted = scenario
        .ordering
        .append(
            "ordering/order",
            "O1",
            "R4",
            None,
            json!({"reference": "a=b c+d", "code": "10"}),
        )
        .await;
    scenario.history.push("R4", domain("ordering", &noted, 9));
    let app = router(tracer(&scenario, true));
    let (status, body) = get(
        &app,
        &format!("{ORDER}?match=ordering%2Fchanged.reference%3Da%3Db+c%2Bd"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(correlation_ids(&body), ["R4"]);
    assert_eq!(body["matched"][0]["value"], "a=b c+d");
    for (value, expected) in [("10", vec!["R4"]), ("10.0", vec![])] {
        let (status, body) = get(
            &app,
            &format!("{ORDER}?match=ordering/changed.code={value}"),
            Some(INSPECTION),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(correlation_ids(&body), expected);
    }
}

#[tokio::test]
async fn match_requires_loaded_payloads_and_a_valid_expression() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));

    let (status, body) = get(
        &app,
        &format!("{ORDER}?match=billing/changed.amount=10"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "match-requires-include");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("include=context:billing"),
        "{body}"
    );
    let (status, body) = get(
        &app,
        &format!("{ORDER}?match=shipping/changed.id=1"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "match-requires-include");

    let deep = format!("ordering/changed.{}=1", ["a"; 9].join("."));
    let long = format!("ordering/changed.id={}", "x".repeat(257));
    let many = ["match=ordering/changed.id=1"; 9].join("&");
    for query in [
        "match=ordering".to_owned(),
        "match=ordering/changed".to_owned(),
        "match=ordering/changed=1".to_owned(),
        "match=ordering/changed.amount".to_owned(),
        "match=ordering/.amount=1".to_owned(),
        "match=ordering/changed..amount=1".to_owned(),
        "match=ordering/changed.amount.=1".to_owned(),
        "match=Ordering%20Context/changed.amount=1".to_owned(),
        "match=/changed.amount=1".to_owned(),
        "match=ordering/changed.id=%FF".to_owned(),
        format!("match={deep}"),
        format!("match={long}"),
        many,
    ] {
        let (status, body) = get(&app, &format!("{ORDER}?{query}"), Some(INSPECTION)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(body["code"], "invalid-match", "{query}");
    }
    assert!(
        scenario.ordering.loads().is_empty(),
        "rejected before loading"
    );
    assert!(
        scenario.history.calls().is_empty(),
        "rejected before retained reads"
    );

    let deepest = format!("ordering/changed.{}=1", ["a"; 8].join("."));
    let longest = format!("ordering/changed.id={}", "x".repeat(256));
    let most = ["match=ordering/changed.id=1"; 8].join("&");
    for query in [format!("match={deepest}"), format!("match={longest}"), most] {
        let (status, body) = get(&app, &format!("{ORDER}?{query}"), Some(INSPECTION)).await;
        assert_eq!(status, StatusCode::OK, "{query}: {body}");
        assert_eq!(body["correlations"], json!([]));
    }

    for token in [CONTROL, DISPATCH] {
        assert_eq!(
            get(
                &app,
                &format!("{ORDER}?match=ordering/changed.placed=true"),
                Some(token),
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn an_anchor_id_without_a_stream_is_not_found_and_an_uncorrelated_stream_is_empty() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));
    let (status, body) = get(
        &app,
        "/aggregates/ordering%2Forder/I1/correlations",
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "unknown-anchor-stream");
    assert!(scenario.history.calls().is_empty());

    let legacy = stream("ordering/order", "O0");
    let operation = OperationId::new("legacy-import").unwrap();
    let commit = derive_commit_id(&legacy, &operation);
    scenario
        .ordering
        .store
        .append(
            &legacy,
            ExpectedVersion::NoStream,
            EventBatch::new(
                commit.clone(),
                operation,
                ContentFingerprint::digest("legacy"),
                vec![
                    NewEvent::new(derive_event_id(&commit, 0), "imported", 1, b"{}".to_vec())
                        .unwrap(),
                ],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let (status, body) = get(
        &app,
        "/aggregates/ordering%2Forder/O0/correlations",
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["correlations"], json!([]));
    assert_snapshot(&body);
    assert_eq!(scenario.history.calls(), [Vec::<String>::new()]);
}

#[tokio::test]
async fn payloads_follow_the_trace_payload_policy() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, false));
    let (status, body) = get(
        &app,
        &format!("{ORDER}?include=context:billing"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for correlation in body["correlations"].as_array().unwrap() {
        for reference in correlation["refs"].as_array().unwrap() {
            assert!(reference.get("payload").is_none(), "{reference}");
        }
    }
    for query in [
        "match=ordering/changed.placed=true",
        "include=context:billing&match=billing/changed.amount=10",
    ] {
        let (status, body) = get(&app, &format!("{ORDER}?{query}"), Some(INSPECTION)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["correlations"],
            json!([]),
            "hidden payloads never match"
        );
    }
}

#[tokio::test]
async fn a_chain_touching_two_anchors_appears_under_both() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));
    let (status, order) = get(&app, ORDER, Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, customer) = get(&app, CUSTOMER, Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(customer["correlations"].as_array().unwrap().len(), 1);
    let shared = &customer["correlations"][0];
    assert_eq!(shared["correlation_id"], "R3");
    assert_eq!(event_ids(shared), event_ids(&order["correlations"][2]));
    assert_eq!(shared["refs"][0]["payload"], json!({"vip": true}));
    assert!(shared["refs"][1].get("payload").is_none());
    assert!(order["correlations"][2]["refs"][0].get("payload").is_none());
    assert_eq!(
        order["correlations"][2]["refs"][1]["payload"],
        json!({"noted": true})
    );
    assert_eq!(
        scenario.history.calls(),
        [vec!["R1", "R2", "R3"], vec!["R3"]]
    );
}

#[tokio::test]
async fn correlation_routes_require_the_production_inspection_capability() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));
    for path in [ORDER, "/correlations/R1/events"] {
        assert_eq!(get(&app, path, None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(
            get(&app, path, Some("invalid")).await.0,
            StatusCode::UNAUTHORIZED
        );
        for token in [CONTROL, DISPATCH] {
            assert_eq!(get(&app, path, Some(token)).await.0, StatusCode::FORBIDDEN);
        }
    }
    assert!(scenario.history.calls().is_empty());
    assert!(scenario.ordering.loads().is_empty());
    for path in [ORDER, "/correlations/R1/events"] {
        assert_eq!(get(&app, path, Some(INSPECTION)).await.0, StatusCode::OK);
    }
    let (status, body) = get(
        &app,
        "/aggregates/ordering%2Fshipment/S1/correlations",
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "unknown-anchor");
    let (status, body) = get(
        &app,
        &format!("{ORDER}?include=context:shipping"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(body["code"], "history-unavailable");
    assert_eq!(
        get(&app, &format!("{ORDER}?expand=all"), Some(INSPECTION))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn the_catalog_lists_anchor_types_for_production_inspection() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));
    let (status, inspection) = get(&app, "/catalog", Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(inspection["contexts"], json!([]));
    assert_eq!(
        inspection["correlations"],
        json!({
            "eventsHrefTemplate": "/correlations/{correlationId}/events",
            "anchors": [
                {
                    "aggregateType": "ordering/customer",
                    "label": "Customer",
                    "correlationsHrefTemplate": "/aggregates/ordering%2Fcustomer/{aggregateId}/correlations",
                },
                {
                    "aggregateType": "ordering/order",
                    "label": "Order",
                    "correlationsHrefTemplate": "/aggregates/ordering%2Forder/{aggregateId}/correlations",
                },
            ],
        })
    );
    let (status, control) = get(&app, "/catalog", Some(CONTROL)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(control.get("correlations").is_none());
    let aggregates = control["contexts"][0]["aggregates"].as_array().unwrap();
    let anchors: Vec<_> = aggregates
        .iter()
        .filter(|aggregate| aggregate["anchor"] == true)
        .map(|aggregate| aggregate["aggregateType"].as_str().unwrap())
        .collect();
    assert_eq!(anchors, ["ordering/customer", "ordering/order"]);
    assert!(
        aggregates
            .iter()
            .any(|aggregate| aggregate.get("anchor").is_none())
    );
    assert_eq!(
        get(&app, "/catalog", Some(DISPATCH)).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn an_inspection_only_host_can_read_and_discover_retained_correlations() {
    let scenario = scenario().await;
    let app = http::router(
        tracer(&scenario, true),
        HttpConfig::inspection_only(INSPECTION).unwrap(),
    );
    for path in [ORDER, "/correlations/R1/events", "/catalog"] {
        let (status, _) = get(&app, path, Some(INSPECTION)).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(
            get(&app, path, Some(CONTROL)).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn without_a_reader_correlation_inspection_is_unavailable_and_not_advertised() {
    let mut registry = DomainRegistry::new();
    registry.register_aggregate::<Order>().unwrap();
    let tracer = TracerBuilder::new(Arc::new(InMemoryEventStore::new()), registry)
        .with_domain_model(domain_model())
        .build()
        .unwrap();
    let app = router(tracer);
    for path in [ORDER, "/correlations/R1/events"] {
        let (status, body) = get(&app, path, Some(INSPECTION)).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["code"], "correlation-history-unavailable");
    }
    let (status, catalog) = get(&app, "/catalog", Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(catalog.get("correlations").is_none());
    for token in [CONTROL, DISPATCH] {
        assert_eq!(
            get(&app, "/correlations/R1/events", Some(token)).await.0,
            StatusCode::NOT_FOUND,
            "the existing operation SSE route still resolves its own correlations"
        );
    }
}

#[tokio::test]
async fn repeated_correlations_and_includes_load_each_stream_once_per_request() {
    let scenario = scenario().await;
    let repeated = scenario
        .ordering
        .append(
            "ordering/order",
            "O1",
            "R1",
            None,
            json!({"repeated": true}),
        )
        .await;
    scenario
        .history
        .push("R1", domain("ordering", &repeated, 9));
    let adjusted = scenario
        .billing
        .append(
            "billing/invoice",
            "I1",
            "R2",
            None,
            json!({"adjusted": true}),
        )
        .await;
    scenario
        .history
        .push("R2", domain("billing", &adjusted, 10));
    let app = router(tracer(&scenario, true));
    let (status, body) = get(
        &app,
        &format!("{ORDER}?include=context:billing,context:ordering&include=context:billing"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(correlation_ids(&body), ["R1", "R2", "R3"]);
    assert_eq!(scenario.history.calls(), [vec!["R1", "R2", "R3"]]);
    assert_eq!(scenario.billing.loads().len(), 2);
    assert_eq!(
        scenario
            .billing
            .loads()
            .into_iter()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            stream("billing/invoice", "I1"),
            stream("billing/invoice", "I2")
        ])
    );
    assert_eq!(scenario.ordering.loads().len(), 3);
    assert_eq!(
        scenario
            .ordering
            .loads()
            .into_iter()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            stream("ordering/order", "O1"),
            stream("ordering/customer", "C7"),
            stream("ordering/shipment", "S1"),
        ])
    );
    assert_eq!(
        body["correlations"][0]["refs"][4]["payload"],
        json!({"repeated": true})
    );
    assert_eq!(
        body["correlations"][1]["refs"][2]["payload"],
        json!({"adjusted": true})
    );
}

#[tokio::test]
async fn anchor_selection_is_bounded_at_read_time_without_losing_older_direct_queries() {
    let scenario = scenario().await;
    for number in 0..257 {
        let id = format!("bounded-{number:03}");
        let event = scenario
            .ordering
            .append("ordering/order", "O2", &id, None, json!({"number": number}))
            .await;
        scenario
            .history
            .push(&id, domain("ordering", &event, number));
    }
    let app = router(tracer(&scenario, true));
    let (status, body) = get(
        &app,
        "/aggregates/ordering%2Forder/O2/correlations",
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["truncated"], true);
    assert_snapshot(&body);
    let selected: Vec<_> = (1..257)
        .map(|number| format!("bounded-{number:03}"))
        .collect();
    assert_eq!(correlation_ids(&body), selected);
    assert_eq!(scenario.history.calls(), [selected]);
    assert!(
        body["correlations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|chain| chain["truncated"] == false),
        "anchor selection truncation is distinct from a truncated individual read"
    );

    let (status, old) = get(&app, "/correlations/bounded-000/events", Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(old["refs"].as_array().unwrap().len(), 1);
    assert_eq!(old["truncated"], false);
}

#[tokio::test]
async fn reader_truncation_is_preserved_on_anchor_and_direct_views() {
    let scenario = scenario().await;
    scenario.history.truncate("R1");
    let app = router(tracer(&scenario, true));
    let (status, body) = get(&app, ORDER, Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["truncated"], true);
    assert_eq!(body["correlations"][0]["truncated"], true);
    assert_eq!(body["correlations"][1]["truncated"], false);
    assert_eq!(body["correlations"][2]["truncated"], false);
    let (status, direct) = get(&app, "/correlations/R1/events", Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(direct["truncated"], true);
    assert_eq!(event_ids(&direct), event_ids(&body["correlations"][0]));
    assert_snapshot(&direct);
}

#[tokio::test]
async fn a_payload_match_cannot_hide_truncation_of_an_excluded_chain() {
    let scenario = scenario().await;
    scenario.history.truncate("R1");
    let app = router(tracer(&scenario, true));
    let (status, body) = get(
        &app,
        &format!("{ORDER}?match=ordering/changed.unretained=true"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["correlations"], json!([]));
    assert_snapshot(&body);
    assert_eq!(scenario.history.calls(), [vec!["R1", "R2", "R3"]]);
    assert_eq!(
        body["truncated"], true,
        "a match cannot establish absence beyond the returned subset of a truncated chain"
    );
}

#[tokio::test]
async fn includes_are_validated_and_bounded_before_reading_history() {
    let scenario = scenario().await;
    let contexts: Vec<_> = (0..17).map(|number| format!("context-{number}")).collect();
    let mut builder = builder(&scenario, true);
    for context in &contexts {
        builder = builder.with_production_history(context, scenario.billing.clone());
    }
    let app = router(builder.build().unwrap());
    for query in [
        "include=all".to_owned(),
        "include=context:".to_owned(),
        "include=context:Ordering".to_owned(),
        "include=context:billing,".to_owned(),
        "include=context:billing&unknown=1".to_owned(),
        contexts
            .iter()
            .map(|context| format!("include=context:{context}"))
            .collect::<Vec<_>>()
            .join("&"),
    ] {
        let (status, body) = get(&app, &format!("{ORDER}?{query}"), Some(INSPECTION)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
    }
    assert!(scenario.ordering.loads().is_empty());
    assert!(scenario.history.calls().is_empty());
    let at_limit = contexts[..16]
        .iter()
        .map(|context| format!("include=context:{context}"))
        .collect::<Vec<_>>()
        .join("&");
    let (status, body) = get(&app, &format!("{ORDER}?{at_limit}"), Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(correlation_ids(&body), ["R1", "R2", "R3"]);
}

#[tokio::test]
async fn included_stream_limit_rejects_the_whole_expansion_instead_of_partial_payloads() {
    let scenario = scenario().await;
    // Two billing streams are already present; 62 more reach the 64-stream boundary.
    for number in 0..62 {
        let event = scenario
            .billing
            .append(
                "billing/invoice",
                &format!("extra-{number}"),
                "R1",
                None,
                json!({"extra": number}),
            )
            .await;
        scenario
            .history
            .push("R1", domain("billing", &event, number + 9));
    }
    let app = router(tracer(&scenario, true));
    let path = format!("{ORDER}?include=context:billing");
    let (status, body) = get(&app, &path, Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(scenario.billing.loads().len(), 64);

    let event = scenario
        .billing
        .append(
            "billing/invoice",
            "too-many",
            "R1",
            None,
            json!({"extra": 62}),
        )
        .await;
    scenario.history.push("R1", domain("billing", &event, 71));
    let (status, body) = get(&app, &path, Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "include-too-broad");
    assert!(body.get("correlations").is_none());
    assert_eq!(
        scenario.billing.loads().len(),
        64,
        "reject before foreign replay"
    );
}
