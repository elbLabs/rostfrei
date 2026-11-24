//! Durable inspection/approval contracts. Run through `cargo xtask test`.
#![allow(clippy::expect_used)]
#[path = "insights/keycloak.rs"]
mod keycloak;
use super::*;
use rostfrei_tracer::insights::{
    AccessDecision, AccessScope, EventHistorySource, Identity, Insights, InsightsError,
    InsightsJournal, NatsInsightsConfig, NatsInsightsJournal, Policy, Principal, Query,
    RequestStatus,
};

fn scope() -> AccessScope {
    AccessScope {
        tenant: "garden".into(),
        application: "shop".into(),
        environment: "production".into(),
    }
}

fn principal(subject: &str, roles: &[&str]) -> Principal {
    Principal::from_trusted_identity(
        Identity {
            issuer: "https://identity.example/realms/garden".into(),
            subject: subject.into(),
            client_id: "support-agent".into(),
        },
        scope(),
        roles.iter().map(|role| (*role).into()).collect(),
    )
    .unwrap()
}

fn reader() -> Principal {
    principal("reader", &["insights.read", "insights.request"])
}

fn admin() -> Principal {
    principal(
        "admin",
        &[
            "insights.policy.manage",
            "insights.approve",
            "insights.audit.read",
        ],
    )
}

fn policy() -> Policy {
    serde_json::from_value(json!({
        "types": [{
            "source": "people",
            "resource": {"kind": "event", "type": "changed", "schemaVersion": 1},
            "record": "requestLess",
            "fields": {
                "/age": "requestLess",
                "/name": "requestOnly",
                "/postaddress": "requestOnly"
            }
        }]
    }))
    .unwrap()
}

fn query() -> Query {
    serde_json::from_value(json!({
        "selection": {
            "source": "people",
            "resource": {"kind": "event", "type": "changed", "schemaVersion": 1},
            "aggregateType": "ordering/customer",
            "aggregateId": "C7"
        },
        "fields": ["/age", "/name", "/postaddress"],
        "purpose": "support-case-42",
        "requestMissing": true
    }))
    .unwrap()
}

async fn journal() -> (
    async_nats::jetstream::Context,
    Arc<NatsInsightsJournal>,
    String,
) {
    let url = std::env::var("ROSTFREI_NATS_URL")
        .expect("run this contract with cargo xtask test and its disposable broker");
    let connection = rostfrei_nats::connect(&rostfrei_nats::NatsConnectionConfig::new(
        "insights-tests",
        url,
    ))
    .await
    .unwrap();
    let js = connection.jetstream().clone();
    let name = format!("INSIGHTS_{}", uuid::Uuid::now_v7().simple());
    let journal = NatsInsightsJournal::provision(js.clone(), &name, scope(), journal_config())
        .await
        .unwrap();
    (js, Arc::new(journal), name)
}

const fn journal_config() -> NatsInsightsConfig {
    NatsInsightsConfig {
        replicas: 1,
        max_bytes: 64 * 1024 * 1024,
        retention: std::time::Duration::from_hours(720),
    }
}

fn service(journal: Arc<dyn InsightsJournal>, history: Arc<CountingHistory>) -> Insights {
    Insights::new(scope(), journal, &[42; 32])
        .unwrap()
        .with_source(
            "people",
            Arc::new(EventHistorySource::new(scope(), history).unwrap()),
        )
        .unwrap()
}

async fn nats_history(js: &async_nats::jetstream::Context) -> Arc<rostfrei_nats::NatsEventStore> {
    let context = rostfrei_messaging_core::BoundedContext::new_in_scope(
        rostfrei_messaging_core::ApplicationName::new(format!(
            "insights-{}",
            uuid::Uuid::now_v7().simple()
        ))
        .unwrap(),
        rostfrei_messaging_core::TrafficScope::Test,
        rostfrei_messaging_core::BoundedContextName::new("ordering").unwrap(),
    );
    let config = rostfrei_nats::NatsEventStoreConfig::for_bounded_context(&context)
        .unwrap()
        .with_fresh_outcomes()
        .unwrap();
    rostfrei_nats::provision_event_store(js, &config)
        .await
        .unwrap();
    Arc::new(
        rostfrei_nats::NatsEventStore::connect(js.clone(), config)
            .await
            .unwrap(),
    )
}

async fn append_customer(history: &rostfrei_nats::NatsEventStore, payload: Value) {
    let stream = stream("ordering/customer", "C7");
    let prior = history.load(&stream).await.unwrap();
    let operation = OperationId::new(format!("change-user-{}", prior.len())).unwrap();
    let commit = derive_commit_id(&stream, &operation);
    let expected = prior.last().map_or(ExpectedVersion::NoStream, |event| {
        ExpectedVersion::Exact(event.stream_version())
    });
    history
        .append_fixture(
            &stream,
            expected,
            EventBatch::new(
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
            .unwrap(),
        )
        .await
        .unwrap();
}

struct Person;

impl Aggregate for Person {
    type State = BTreeMap<String, Value>;
    type Event = BTreeMap<String, Value>;
    const BOUNDED_CONTEXT: &'static str = "ordering";
    const AGGREGATE_TYPE: &'static str = "ordering/customer";
    fn initial(_: &StreamId) -> Self::State {
        BTreeMap::new()
    }
    fn apply(state: &mut Self::State, event: &Self::Event) {
        state.extend(event.clone());
    }
}

struct PersonCodec;

impl rostfrei_core::EventCodec<Person> for PersonCodec {
    fn encode(
        &self,
        event: &BTreeMap<String, Value>,
        id: rostfrei_core::EventId,
    ) -> Result<NewEvent, rostfrei_core::EventCodecError> {
        Ok(NewEvent::new(id, "changed", 1, serde_json::to_vec(event).unwrap()).unwrap())
    }

    fn decode(
        &self,
        event: &RecordedEvent,
    ) -> Result<BTreeMap<String, Value>, rostfrei_core::EventCodecError> {
        serde_json::from_slice(event.payload()).map_err(|error| {
            rostfrei_core::EventCodecError::new(
                rostfrei_core::EventCodecErrorKind::MalformedPayload,
                error.to_string(),
            )
        })
    }
}

#[tokio::test]
async fn aggregate_approvals_bind_rehydrated_versions_and_source_scope() {
    use rostfrei_tracer::insights::{AggregateStateSource, InsightsSource as _, ResourceKind};

    let (js, journal, name) = journal().await;
    let history = nats_history(&js).await;
    append_customer(&history, json!({"name":"Alice","age":33})).await;
    append_customer(&history, json!({"age":34})).await;
    let source = Arc::new(
        AggregateStateSource::<Person>::new(scope(), history.clone(), Arc::new(PersonCodec), 1)
            .unwrap(),
    );
    let insights = Insights::new(scope(), journal, &[42; 32])
        .unwrap()
        .with_source("people", source.clone())
        .unwrap();
    let mut query = query();
    query.selection.resource.kind = ResourceKind::Aggregate;
    query.selection.resource.type_name = "ordering/customer".into();
    let mut policy = policy();
    policy.types[0].resource = query.selection.resource.clone();
    insights
        .set_policy(&admin(), 0, policy.clone())
        .await
        .unwrap();
    let first = insights.query(&reader(), query.clone()).await.unwrap();
    assert_eq!(
        first.records[0].fields,
        BTreeMap::from([("/age".into(), json!(34))])
    );
    let id = first.request_id.unwrap();
    let decision = AccessDecision {
        approve: true,
        duration_seconds: 300,
        fields: None,
        records: None,
    };
    assert_eq!(
        insights
            .decide(
                &principal("reader", &["insights.approve"]),
                &id,
                decision.clone()
            )
            .await,
        Err(InsightsError::Forbidden),
        "requesters cannot approve their own access",
    );
    insights.decide(&admin(), &id, decision).await.unwrap();
    query.request_id = Some(id);
    let approved = insights.query(&reader(), query.clone()).await.unwrap();
    assert_eq!(
        approved.records[0].fields["/name"], "Alice",
        "state replays both events"
    );
    let other_scope = AccessScope {
        tenant: "another-tenant".into(),
        ..scope()
    };
    assert!(matches!(
        source.read(&other_scope, &query.selection).await,
        Err(InsightsError::Forbidden)
    ));

    append_customer(&history, json!({"name":"Bob"})).await;
    assert_eq!(
        insights.query(&reader(), query.clone()).await,
        Err(InsightsError::SourceChanged)
    );
    insights.set_policy(&admin(), 1, policy).await.unwrap();
    assert_eq!(
        insights.query(&reader(), query).await,
        Err(InsightsError::Forbidden)
    );
    js.delete_stream(name).await.unwrap();
}

#[tokio::test]
async fn partial_disclosure_approval_and_revocation_survive_service_recreation() {
    let (js, journal, name) = journal().await;
    let history = Arc::new(CountingHistory::default());
    history
        .append(
            "ordering/customer", "C7", "user-created", None,
            json!({"age": 34, "name": "Alice", "postaddress": {"city": "Hamburg"}, "newField": "secret"}),
        )
        .await;
    let insights = service(journal.clone(), history.clone());
    insights.set_policy(&admin(), 0, policy()).await.unwrap();

    let first = insights.query(&reader(), query()).await.unwrap();
    assert_eq!(first.records.len(), 1);
    assert_eq!(
        first.records[0].fields,
        BTreeMap::from([("/age".into(), json!(34))])
    );
    let request_id = first.request_id.unwrap();
    let pending = insights
        .access_request(&admin(), &request_id)
        .await
        .unwrap();
    assert_eq!(pending.status, RequestStatus::Pending);
    assert_eq!(pending.records.len(), 1);
    let duplicate = insights.query(&reader(), query()).await.unwrap();
    assert_eq!(duplicate.request_id.as_deref(), Some(request_id.as_str()));

    // A second instance proves grants are durable, rather than process-local.
    let reopened = Arc::new(
        NatsInsightsJournal::connect(js.clone(), &name, scope())
            .await
            .unwrap(),
    );
    let insights = service(reopened, history.clone());
    insights
        .decide(
            &admin(),
            &request_id,
            AccessDecision {
                approve: true,
                duration_seconds: 300,
                fields: None,
                records: None,
            },
        )
        .await
        .unwrap();

    // Newly matching records are outside the approved frozen result set.
    history
        .append(
            "ordering/customer",
            "C7",
            "user-updated",
            None,
            json!({"age": 35, "name": "Alice", "postaddress": {"city": "Berlin"}}),
        )
        .await;
    let mut approved_query = query();
    approved_query.request_id = Some(request_id.clone());
    let approved = insights
        .query(&reader(), approved_query.clone())
        .await
        .unwrap();
    assert_eq!(approved.records.len(), 1);
    assert_eq!(approved.records[0].fields["/name"], "Alice");
    assert_eq!(
        approved.records[0].fields["/postaddress"],
        json!({"city": "Hamburg"})
    );
    assert!(!approved.records[0].fields.contains_key("/newField"));
    assert!(matches!(
        insights
            .query(
                &principal("other", &["insights.read"]),
                approved_query.clone()
            )
            .await,
        Err(InsightsError::Forbidden)
    ));
    insights.revoke(&admin(), &request_id).await.unwrap();
    assert!(matches!(
        insights.query(&reader(), approved_query).await,
        Err(InsightsError::Forbidden)
    ));

    let audit = insights.audit(&admin(), None, 100).await.unwrap();
    let disclosure = audit
        .entries
        .iter()
        .find(|entry| entry.id == first.audit_id)
        .unwrap();
    assert_eq!(disclosure.identity.subject, "reader");
    assert!(disclosure.timestamp_ms > 0);
    assert_eq!(disclosure.disclosures[0].fields, ["/age"]);
    assert_eq!(
        serde_json::to_value(disclosure).unwrap()["selection"],
        serde_json::to_value(query().selection).unwrap()
    );
    let serialized = serde_json::to_string(&audit).unwrap();
    assert!(!serialized.contains("Alice"));
    assert!(!serialized.contains("Hamburg"));
    assert!(
        audit
            .entries
            .iter()
            .any(|entry| entry.action == "access.revoked")
    );
    js.delete_stream(name).await.unwrap();
}

#[tokio::test]
async fn protected_predicates_do_not_disclose_matches_before_approval() {
    let (js, journal, name) = journal().await;
    let history = Arc::new(CountingHistory::default());
    history
        .append(
            "ordering/customer",
            "C7",
            "user-created",
            None,
            json!({"age": 34, "name": "Alice"}),
        )
        .await;
    let insights = service(journal.clone(), history.clone());
    insights.set_policy(&admin(), 0, policy()).await.unwrap();
    let mut query = query();
    query.fields = vec!["/age".into()];
    query.filters =
        serde_json::from_value(json!([{"path": "/name", "op": "eq", "value": "Alice"}])).unwrap();
    let first = insights.query(&reader(), query.clone()).await.unwrap();
    assert!(
        first.records.is_empty(),
        "even a matching age would reveal a protected name"
    );
    let id = first.request_id.unwrap();
    insights
        .decide(
            &admin(),
            &id,
            AccessDecision {
                approve: true,
                duration_seconds: 300,
                fields: None,
                records: None,
            },
        )
        .await
        .unwrap();
    query.request_id = Some(id);
    let permitted = insights.query(&reader(), query.clone()).await.unwrap();
    assert_eq!(
        permitted.records[0].fields,
        BTreeMap::from([("/age".into(), json!(34))])
    );
    let rotated = Insights::new(scope(), journal, &[43; 32])
        .unwrap()
        .with_source(
            "people",
            Arc::new(EventHistorySource::new(scope(), history).unwrap()),
        )
        .unwrap();
    assert_eq!(
        rotated.query(&reader(), query).await,
        Err(InsightsError::Forbidden),
        "an old query binding must not survive a binding-key rotation"
    );
    js.delete_stream(name).await.unwrap();
}

#[tokio::test]
async fn audit_authority_loss_never_falls_back_to_an_unaudited_read() {
    let (js, journal, name) = journal().await;
    let history = Arc::new(CountingHistory::default());
    history
        .append("ordering/customer", "C7", "R1", None, json!({"age":34}))
        .await;
    let insights = service(journal, history);
    insights.set_policy(&admin(), 0, policy()).await.unwrap();
    js.delete_stream(&name).await.unwrap();
    // Even a recreated stream with the same name cannot restore an old authority.
    NatsInsightsJournal::provision(js.clone(), &name, scope(), journal_config())
        .await
        .unwrap();
    assert!(matches!(
        insights.query(&reader(), query()).await,
        Err(InsightsError::Unavailable)
    ));
    js.delete_stream(name).await.unwrap();
}

#[tokio::test]
async fn explicit_denial_is_not_requestable_and_record_approval_does_not_override_it() {
    let (js, journal, name) = journal().await;
    let history = Arc::new(CountingHistory::default());
    history
        .append(
            "ordering/customer",
            "C7",
            "R1",
            None,
            json!({"age":34,"name":"Alice","password":"never disclose"}),
        )
        .await;
    let insights = service(journal, history);
    let mut policy = policy();
    policy.types[0].fields.insert(
        "/password".into(),
        rostfrei_tracer::insights::AccessMode::Deny,
    );
    insights
        .set_policy(&admin(), 0, policy.clone())
        .await
        .unwrap();
    let mut only_denied = query();
    only_denied.fields = vec!["/password".into()];
    let denied = insights.query(&reader(), only_denied).await.unwrap();
    assert!(
        denied.request_id.is_none(),
        "deny must not create an approvable request"
    );
    assert!(denied.records.iter().all(|row| row.fields.is_empty()));

    policy.types[0].record = rostfrei_tracer::insights::AccessMode::RequestOnly;
    policy.types[0].fields.insert(
        "/name".into(),
        rostfrei_tracer::insights::AccessMode::RequestLess,
    );
    insights.set_policy(&admin(), 1, policy).await.unwrap();
    let mut protected = query();
    protected.fields = vec!["/age".into(), "/name".into(), "/password".into()];
    let pending = insights.query(&reader(), protected.clone()).await.unwrap();
    assert!(
        pending.records.is_empty(),
        "a requestLess child cannot bypass a protected record"
    );
    let request_id = pending.request_id.unwrap();
    insights
        .decide(
            &admin(),
            &request_id,
            AccessDecision {
                approve: true,
                duration_seconds: 300,
                fields: Some(vec!["/age".into()]),
                records: None,
            },
        )
        .await
        .unwrap();
    protected.request_id = Some(request_id);
    let permitted = insights.query(&reader(), protected).await.unwrap();
    assert_eq!(
        permitted.records[0].fields,
        BTreeMap::from([("/age".into(), json!(34))])
    );
    js.delete_stream(name).await.unwrap();
}

struct DisclosureGate {
    inner: Arc<NatsInsightsJournal>,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    armed: std::sync::atomic::AtomicBool,
    fail: bool,
}

struct Clock(std::sync::atomic::AtomicU64);

impl rostfrei_tracer::insights::InsightsClock for Clock {
    fn now_ms(&self) -> Result<u64, InsightsError> {
        Ok(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }
}

struct DelayedAcknowledgement {
    inner: Arc<NatsInsightsJournal>,
    clock: Arc<Clock>,
}

#[async_trait]
impl InsightsJournal for DelayedAcknowledgement {
    async fn load(&self) -> Result<rostfrei_tracer::insights::JournalSnapshot, InsightsError> {
        self.inner.load().await
    }
    async fn commit(
        &self,
        revision: u64,
        state: Option<rostfrei_tracer::insights::InsightsState>,
        audit: rostfrei_tracer::insights::AuditEntry,
    ) -> Result<bool, InsightsError> {
        let disclosure = audit.action == "query.disclosure";
        let committed = self.inner.commit(revision, state, audit).await?;
        if committed && disclosure {
            self.clock
                .0
                .fetch_add(301_000, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(committed)
    }
    async fn audit(
        &self,
        before: Option<u64>,
        limit: usize,
    ) -> Result<rostfrei_tracer::insights::AuditPage, InsightsError> {
        self.inner.audit(before, limit).await
    }
}

#[tokio::test]
async fn expiry_during_durable_admission_never_releases_an_expired_grant() {
    let (js, journal, name) = journal().await;
    let history = Arc::new(CountingHistory::default());
    history
        .append(
            "ordering/customer",
            "C7",
            "R1",
            None,
            json!({"age":34,"name":"Alice"}),
        )
        .await;
    let clock = Arc::new(Clock(std::sync::atomic::AtomicU64::new(1_800_000_000_000)));
    let insights = service(journal.clone(), history.clone()).with_clock(clock.clone());
    insights.set_policy(&admin(), 0, policy()).await.unwrap();
    let first = insights.query(&reader(), query()).await.unwrap();
    let id = first.request_id.unwrap();
    insights
        .decide(
            &admin(),
            &id,
            AccessDecision {
                approve: true,
                duration_seconds: 300,
                fields: None,
                records: None,
            },
        )
        .await
        .unwrap();
    let gate = Arc::new(DelayedAcknowledgement {
        inner: journal,
        clock: clock.clone(),
    });
    let gated = service(gate, history).with_clock(clock);
    let mut query = query();
    query.request_id = Some(id);
    assert_eq!(
        gated.query(&reader(), query).await,
        Err(InsightsError::Forbidden)
    );
    js.delete_stream(name).await.unwrap();
}

#[tokio::test]
async fn identity_expiry_during_admission_withholds_requestless_data() {
    let (js, journal, name) = journal().await;
    let history = Arc::new(CountingHistory::default());
    history
        .append("ordering/customer", "C7", "R1", None, json!({"age":34}))
        .await;
    let clock = Arc::new(Clock(std::sync::atomic::AtomicU64::new(1_800_000_000_000)));
    let insights = service(journal.clone(), history.clone()).with_clock(clock.clone());
    insights.set_policy(&admin(), 0, policy()).await.unwrap();
    let gate = Arc::new(DelayedAcknowledgement {
        inner: journal.clone(),
        clock: clock.clone(),
    });
    let gated = service(gate, history).with_clock(clock);
    let reader = reader().with_valid_until_ms(1_800_000_300_000);
    let mut query = query();
    query.fields = vec!["/age".into()];
    query.request_missing = false;
    assert_eq!(
        gated.query(&reader, query).await,
        Err(InsightsError::Unauthenticated)
    );
    let audit = journal.audit(None, 100).await.unwrap();
    assert!(
        audit
            .entries
            .iter()
            .any(|entry| entry.action == "query.finished"
                && entry.outcome == InsightsError::Unauthenticated.to_string())
    );
    js.delete_stream(name).await.unwrap();
}

#[async_trait]
impl InsightsJournal for DisclosureGate {
    async fn load(&self) -> Result<rostfrei_tracer::insights::JournalSnapshot, InsightsError> {
        self.inner.load().await
    }
    async fn commit(
        &self,
        expected: u64,
        state: Option<rostfrei_tracer::insights::InsightsState>,
        audit: rostfrei_tracer::insights::AuditEntry,
    ) -> Result<bool, InsightsError> {
        if audit.action == "query.disclosure"
            && self.armed.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.entered.notify_one();
            self.release.notified().await;
            if self.fail {
                return Err(InsightsError::Unavailable);
            }
        }
        self.inner.commit(expected, state, audit).await
    }
    async fn audit(
        &self,
        before: Option<u64>,
        limit: usize,
    ) -> Result<rostfrei_tracer::insights::AuditPage, InsightsError> {
        self.inner.audit(before, limit).await
    }
}

#[tokio::test]
async fn revocation_racing_disclosure_rechecks_authority_before_releasing_fields() {
    let (js, journal, name) = journal().await;
    let history = Arc::new(CountingHistory::default());
    history
        .append(
            "ordering/customer",
            "C7",
            "R1",
            None,
            json!({"age":34,"name":"Alice"}),
        )
        .await;
    let insights = service(journal.clone(), history.clone());
    insights.set_policy(&admin(), 0, policy()).await.unwrap();
    let first = insights.query(&reader(), query()).await.unwrap();
    let id = first.request_id.unwrap();
    insights
        .decide(
            &admin(),
            &id,
            AccessDecision {
                approve: true,
                duration_seconds: 300,
                fields: None,
                records: None,
            },
        )
        .await
        .unwrap();
    let gate = Arc::new(DisclosureGate {
        inner: journal,
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        armed: true.into(),
        fail: false,
    });
    let gated = service(gate.clone(), history);
    let mut query = query();
    query.request_id = Some(id.clone());
    let read = tokio::spawn(async move { gated.query(&reader(), query).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
    assert!(
        !read.is_finished(),
        "no response before durable disclosure admission"
    );
    insights.revoke(&admin(), &id).await.unwrap();
    gate.release.notify_one();
    assert_eq!(read.await.unwrap(), Err(InsightsError::Forbidden));
    let audit = insights.audit(&admin(), None, 100).await.unwrap();
    assert_eq!(
        audit
            .entries
            .iter()
            .filter(|entry| entry.action == "query.disclosure")
            .count(),
        1
    );
    js.delete_stream(name).await.unwrap();
}

#[tokio::test]
async fn publication_failure_after_projection_withholds_even_requestless_data() {
    let (js, journal, name) = journal().await;
    let history = Arc::new(CountingHistory::default());
    history
        .append("ordering/customer", "C7", "R1", None, json!({"age":34}))
        .await;
    let insights = service(journal.clone(), history.clone());
    insights.set_policy(&admin(), 0, policy()).await.unwrap();
    let gate = Arc::new(DisclosureGate {
        inner: journal,
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        armed: true.into(),
        fail: true,
    });
    let gated = service(gate.clone(), history);
    let mut query = query();
    query.fields = vec!["/age".into()];
    query.request_missing = false;
    let read = tokio::spawn(async move { gated.query(&reader(), query).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
    assert!(!read.is_finished());
    gate.release.notify_one();
    assert_eq!(read.await.unwrap(), Err(InsightsError::Unavailable));
    let audit = insights.audit(&admin(), None, 100).await.unwrap();
    assert!(
        !audit
            .entries
            .iter()
            .any(|entry| entry.action == "query.disclosure")
    );
    js.delete_stream(name).await.unwrap();
}

#[tokio::test]
async fn retained_audit_expiry_never_resets_authorization_to_an_empty_state() {
    let (js, _, name) = journal().await;
    js.delete_stream(&name).await.unwrap();
    let mut config = journal_config();
    config.retention = std::time::Duration::from_millis(150);
    let journal = NatsInsightsJournal::provision(js.clone(), &name, scope(), config.clone())
        .await
        .unwrap();
    let mut stream = js.get_stream(&name).await.unwrap();
    assert_eq!(stream.cached_info().config.max_age, config.retention);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if stream.info().await.unwrap().state.messages == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        journal.load().await,
        Err(InsightsError::Unavailable)
    ));
    assert!(matches!(
        NatsInsightsJournal::connect(js.clone(), &name, scope()).await,
        Err(InsightsError::Unavailable)
    ));
    js.delete_stream(name).await.unwrap();
}
