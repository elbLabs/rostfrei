#![cfg(feature = "http")]
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::{
    collections::{BTreeMap, HashMap},
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
    CorrelationIndexLag, CorrelationIndexReader, CorrelationRef, DomainEventRef, EventBatch,
    EventHistory, EventStore, EventStoreError, ExpectedVersion, InMemoryEventStore,
    IntegrationEventRef, NewEvent, OperationId, ReadModelError, RecordedEvent, StreamId,
    derive_commit_id, derive_event_id,
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
        let stream = StreamId::new(
            AggregateType::new(aggregate_type).unwrap(),
            AggregateId::new(aggregate_id).unwrap(),
        );
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

#[derive(Default)]
struct FakeIndex {
    chains: Mutex<HashMap<String, CorrelationChain>>,
    lag: Mutex<CorrelationIndexLag>,
    expired: Mutex<Vec<String>>,
}

impl FakeIndex {
    fn push(&self, correlation: &str, reference: CorrelationRef) {
        self.chains
            .lock()
            .unwrap()
            .entry(correlation.to_owned())
            .or_default()
            .refs
            .push(reference);
    }
}

#[async_trait]
impl CorrelationIndexReader for FakeIndex {
    async fn chain(
        &self,
        correlation_id: &CorrelationId,
    ) -> Result<CorrelationChain, ReadModelError> {
        Ok(self
            .chains
            .lock()
            .unwrap()
            .get(correlation_id.as_str())
            .cloned()
            .unwrap_or_default())
    }

    async fn lag(&self) -> Result<CorrelationIndexLag, ReadModelError> {
        Ok(self.lag.lock().unwrap().clone())
    }

    async fn integration_available(
        &self,
        reference: &IntegrationEventRef,
    ) -> Result<bool, ReadModelError> {
        Ok(!self.expired.lock().unwrap().contains(&reference.message_id))
    }
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
        sequence: 7,
    })
}

struct Scenario {
    ordering: Arc<CountingHistory>,
    billing: Arc<CountingHistory>,
    index: Arc<FakeIndex>,
    events: BTreeMap<&'static str, RecordedEvent>,
}

/// R1: order O1 placed -> integration event -> billing invoice I1 -> shipment S1.
/// R2: order O1 amended, billing invoice I2. R3: customer C7 updated, order O1 noted.
async fn scenario() -> Scenario {
    let ordering = Arc::new(CountingHistory::default());
    let billing = Arc::new(CountingHistory::default());
    let index = Arc::new(FakeIndex::default());
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
    index.push("R1", domain("ordering", &placed, 1));
    index.push("R1", integration(&placed, "message-1", 2));
    index.push("R1", domain("billing", &invoiced, 3));
    index.push("R1", domain("ordering", &shipped, 4));
    index.push("R2", domain("ordering", &amended, 5));
    index.push("R2", domain("billing", &reinvoiced, 6));
    index.push("R3", domain("ordering", &updated, 7));
    index.push("R3", domain("ordering", &noted, 8));
    *index.lag.lock().unwrap() =
        BTreeMap::from([("billing".to_owned(), 0), ("ordering".to_owned(), 0)]);
    events.insert("placed", placed);
    events.insert("invoiced", invoiced);
    events.insert("amended", amended);
    events.insert("updated", updated);
    events.insert("noted", noted);
    Scenario {
        ordering,
        billing,
        index,
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

fn tracer(scenario: &Scenario, expose: bool) -> Tracer {
    let mut registry = DomainRegistry::new();
    registry.register_aggregate::<Order>().unwrap();
    registry.register_aggregate::<Customer>().unwrap();
    registry.register_aggregate::<Shipment>().unwrap();
    let mut builder = TracerBuilder::new(Arc::new(InMemoryEventStore::new()), registry)
        .with_domain_model(domain_model())
        .with_correlation_index(scenario.index.clone())
        .with_production_history("ordering", scenario.ordering.clone())
        .with_production_history("billing", scenario.billing.clone());
    if expose {
        builder =
            builder.with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    }
    builder.build().unwrap()
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
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
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
    assert_eq!(body["lag"], json!({"billing": 0, "ordering": 0}));
    assert!(body.get("truncated").is_none());
    let correlations = body["correlations"].as_array().unwrap();
    assert_eq!(
        correlations
            .iter()
            .map(|correlation| correlation["correlation_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["R1", "R2", "R3"]
    );
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
            "available": true,
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
    let order = StreamId::new(
        AggregateType::new("ordering/order").unwrap(),
        AggregateId::new("O1").unwrap(),
    );
    assert_eq!(scenario.ordering.loads(), std::slice::from_ref(&order));
    assert!(scenario.billing.loads().is_empty());

    let (status, body) = get(
        &app,
        &format!("{ORDER}?include=context:billing"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
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

    let legacy = StreamId::new(
        AggregateType::new("ordering/order").unwrap(),
        AggregateId::new("O0").unwrap(),
    );
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
}

#[tokio::test]
async fn a_chain_touching_two_anchors_appears_under_both() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));
    let (_, order) = get(&app, ORDER, Some(INSPECTION)).await;
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
}

#[tokio::test]
async fn expired_integration_messages_remain_in_the_chain_as_unavailable() {
    let scenario = scenario().await;
    scenario
        .index
        .expired
        .lock()
        .unwrap()
        .push("message-1".to_owned());
    let app = router(tracer(&scenario, true));
    let (status, body) = get(&app, "/correlations/R1/events", Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["correlation_id"], "R1");
    assert_eq!(body["lag"], json!({"billing": 0, "ordering": 0}));
    assert_eq!(body["refs"].as_array().unwrap().len(), 4);
    assert_eq!(body["refs"][1]["kind"], "integration");
    assert_eq!(body["refs"][1]["available"], false);
    assert!(body["refs"][0].get("payload").is_none());
}

#[tokio::test]
async fn correlation_routes_require_the_production_inspection_capability() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));
    for path in [ORDER, "/correlations/R1/events"] {
        assert_eq!(get(&app, path, None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(
            get(&app, path, Some(CONTROL)).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            get(&app, path, Some(DISPATCH)).await.0,
            StatusCode::FORBIDDEN
        );
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
    let (status, body) = get(&app, "/correlations/unknown/events", Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not-found");
    scenario
        .index
        .lag
        .lock()
        .unwrap()
        .insert("billing".to_owned(), 3);
    let (status, body) = get(&app, "/correlations/unknown/events", Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["refs"], json!([]));
    assert_eq!(body["lag"]["billing"], 3);
    let (status, body) = get(
        &app,
        &format!("{ORDER}?include=context:shipping"),
        Some(INSPECTION),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(body["code"], "history-unavailable");
    let (status, _) = get(&app, &format!("{ORDER}?expand=all"), Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_catalog_lists_anchor_types_for_production_inspection() {
    let scenario = scenario().await;
    let app = router(tracer(&scenario, true));
    let (_, inspection) = get(&app, "/catalog", Some(INSPECTION)).await;
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
    let (_, control) = get(&app, "/catalog", Some(CONTROL)).await;
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
}

#[tokio::test]
async fn without_an_index_correlation_inspection_is_unavailable() {
    let mut registry = DomainRegistry::new();
    registry.register_aggregate::<Order>().unwrap();
    let tracer = TracerBuilder::new(Arc::new(InMemoryEventStore::new()), registry)
        .with_domain_model(domain_model())
        .build()
        .unwrap();
    let app = router(tracer);
    let (status, body) = get(&app, ORDER, Some(INSPECTION)).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(body["code"], "correlation-index-unavailable");
    assert_eq!(
        get(&app, "/correlations/R1/events", Some(CONTROL)).await.0,
        StatusCode::NOT_FOUND
    );
}
