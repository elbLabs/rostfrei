//! Future-API contracts for marker/details separation and independently implemented readers.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
use async_trait::async_trait;
use rostfrei_core::*;
use rostfrei_messaging_core::BoundedContextName;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn metadata(id: &str) -> CommandExecutionMetadata {
    CommandExecutionMetadata::new(
        OperationId::new(id).unwrap(),
        ContentFingerprint::digest(id),
    )
    .with_bounded_context(BoundedContextName::new("lifecycle").unwrap())
}
fn key(store: &InMemoryEventStore, id: &str) -> OutcomeKey {
    OutcomeKey::new(
        store.authority().clone(),
        BoundedContextName::new("lifecycle").unwrap(),
        OperationId::new(id).unwrap(),
    )
}
struct Decide {
    calls: Arc<AtomicUsize>,
    reject: bool,
}
#[async_trait]
impl CommandHandler<()> for Decide {
    type Rejection = String;
    async fn handle(&self, (): &(), _: &mut CommandExecution<'_>) -> CommandHandlingResult<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(if self.reject {
            CommandDecision::Rejected("secret exact rejection".to_owned())
        } else {
            CommandDecision::Accepted
        })
    }
}
async fn execute(
    store: InMemoryEventStore,
    handler: &Decide,
    id: &str,
) -> Result<CommandOutcome<String>, CommandExecutionError> {
    CommandExecutor::new(store)
        .with_max_conflict_retries(0)
        .execute_with_rejection_codec(
            handler,
            metadata(id),
            &(),
            &JsonRejectionCodec::new("lifecycle.rejection", 1),
        )
        .await
}

/// Independent test adapter: stores ONLY exact detached bytes and records the stable reference.
/// It has no `EventStore`, journal, transaction or completion accessor.
struct Reader {
    response: Mutex<Result<DetailRead, DetailReadError>>,
    requests: Mutex<Vec<OutcomeDetailReference>>,
}
impl Reader {
    fn new(response: Result<DetailRead, DetailReadError>) -> Arc<Self> {
        Arc::new(Self {
            response: Mutex::new(response),
            requests: Mutex::new(Vec::new()),
        })
    }
}
#[async_trait]
impl OutcomeDetailReader for Reader {
    async fn get(&self, reference: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        self.requests.lock().unwrap().push(reference.clone());
        self.response.lock().unwrap().clone()
    }
}
/// A separately owned keyed cold backend. It has no reference to the hot store or
/// journal and returns absence for any reference that was not explicitly copied.
struct ColdStore {
    entries: Mutex<Vec<(OutcomeDetailReference, Vec<u8>)>>,
    requests: Mutex<Vec<OutcomeDetailReference>>,
}
impl ColdStore {
    fn copied(reference: OutcomeDetailReference, bytes: Vec<u8>) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(vec![(reference, bytes)]),
            requests: Mutex::new(Vec::new()),
        })
    }
}
#[async_trait]
impl OutcomeDetailReader for ColdStore {
    async fn get(&self, reference: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        self.requests.lock().unwrap().push(reference.clone());
        Ok(self
            .entries
            .lock()
            .unwrap()
            .iter()
            .find(|(key, _)| key == reference)
            .map_or(DetailRead::AbsentHere, |(_, bytes)| {
                DetailRead::Found(bytes.clone())
            }))
    }
}
async fn completed(
    store: &InMemoryEventStore,
    id: &str,
) -> TestResult<CompletedOutcome<EncodedOutcomeDetails>> {
    match store.lookup_outcome(&key(store, id)).await? {
        OutcomeLookup::Completed(outcome) => Ok(outcome),
        OutcomeLookup::Missing => Err("completed decision was lost".into()),
    }
}
async fn detached_bytes(
    store: &InMemoryEventStore,
    id: &str,
) -> TestResult<(OutcomeDetailReference, Vec<u8>)> {
    let reference = completed(store, id)
        .await?
        .marker()
        .detail_reference()
        .clone();
    let DetailRead::Found(bytes) = store.detail_reader().get(&reference).await? else {
        return Err("fresh detail not prepared".into());
    };
    Ok((reference, bytes))
}

#[tokio::test]
async fn independent_cold_reader_recovers_the_same_reference_after_hot_absence_or_outage()
-> TestResult {
    let store = InMemoryEventStore::new();
    let handler = Decide {
        calls: Arc::new(AtomicUsize::new(0)),
        reject: true,
    };
    let expected = execute(store.clone(), &handler, "cold").await?;
    let (reference, bytes) = detached_bytes(&store, "cold").await?;
    for hot_response in [
        Ok(DetailRead::AbsentHere),
        Err(DetailReadError::Unavailable),
    ] {
        let hot = Reader::new(hot_response);
        let cold = ColdStore::copied(reference.clone(), bytes.clone());
        let reader = store
            .clone()
            .with_detail_readers(hot.clone(), Some(cold.clone()));
        assert_eq!(execute(reader, &handler, "cold").await?, expected);
        for requests in [&hot.requests, &cold.requests] {
            let requests = requests.lock().unwrap();
            assert!(!requests.is_empty());
            assert!(requests.iter().all(|requested| requested == &reference));
            drop(requests);
        }
    }
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn missing_unavailable_corrupt_and_forbidden_details_never_become_executable_absence()
-> TestResult {
    let store = InMemoryEventStore::new();
    let handler = Decide {
        calls: Arc::new(AtomicUsize::new(0)),
        reject: true,
    };
    execute(store.clone(), &handler, "blocked").await?;
    let (reference, bytes) = detached_bytes(&store, "blocked").await?;
    for (hot, cold, reason) in [
        (
            Ok(DetailRead::AbsentHere),
            Ok(DetailRead::AbsentHere),
            DetailUnavailableReason::MissingDetails,
        ),
        (
            Ok(DetailRead::AbsentHere),
            Err(DetailReadError::Unavailable),
            DetailUnavailableReason::BackendUnavailable,
        ),
        (
            Err(DetailReadError::Unavailable),
            Ok(DetailRead::AbsentHere),
            DetailUnavailableReason::BackendUnavailable,
        ),
    ] {
        let reader = store
            .clone()
            .with_detail_readers(Reader::new(hot), Some(Reader::new(cold)));
        assert!(
            matches!(completed(&reader, "blocked").await?.details(), OutcomeDetails::Unavailable { reason: actual } if *actual == reason)
        );
        assert!(execute(reader, &handler, "blocked").await.is_err());
    }
    let mut corrupt = bytes.clone();
    corrupt.push(0);
    for response in [
        Ok(DetailRead::Found(corrupt)),
        Err(DetailReadError::Forbidden),
    ] {
        let cold = ColdStore::copied(reference.clone(), bytes.clone());
        let reader = store
            .clone()
            .with_detail_readers(Reader::new(response), Some(cold.clone()));
        assert!(
            reader
                .lookup_outcome(&key(&reader, "blocked"))
                .await
                .is_err()
        );
        assert!(execute(reader, &handler, "blocked").await.is_err());
        assert!(
            cold.requests.lock().unwrap().is_empty(),
            "permission or corruption cannot be bypassed through cold fallback"
        );
    }
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        store.lookup_outcome(&key(&store, "genuinely-new")).await?,
        OutcomeLookup::Missing
    ));
    Ok(())
}

#[tokio::test]
async fn expiry_is_revision_bound_reclaims_hot_bytes_and_overrides_a_still_present_cold_copy()
-> TestResult {
    for reject in [false, true] {
        let store = InMemoryEventStore::new();
        let handler = Decide {
            calls: Arc::new(AtomicUsize::new(0)),
            reject,
        };
        execute(store.clone(), &handler, "expire").await?;
        execute(store.clone(), &handler, "unrelated").await?;
        let retained = completed(&store, "expire").await?;
        let unrelated = completed(&store, "unrelated").await?;
        let (reference, bytes) = detached_bytes(&store, "expire").await?;
        assert!(
            store
                .expire_details(&key(&store, "expire"), unrelated.marker().revision())
                .await
                .is_err(),
            "an operation-only or other-reference revision must not be accepted"
        );
        assert!(matches!(
            completed(&store, "expire").await?.details(),
            OutcomeDetails::Available(_)
        ));
        let cold = ColdStore::copied(reference.clone(), bytes);
        let resolver = store
            .clone()
            .with_detail_readers(store.detail_reader(), Some(cold.clone()));
        let expiry = store
            .expire_details(&key(&store, "expire"), retained.marker().revision())
            .await?;
        assert_eq!(expiry.revision(), retained.marker().revision());
        assert_eq!(expiry.reclamation(), ReclamationStatus::Reclaimed);
        // Same request is idempotent and may finish reclamation after an interrupted declaration.
        store
            .expire_details(&key(&store, "expire"), retained.marker().revision())
            .await?;
        assert!(
            matches!(
                store.detail_reader().get(&reference).await?,
                DetailRead::AbsentHere
            ),
            "expiry must delete independently stored bytes"
        );
        let expired = completed(&resolver, "expire").await?;
        assert_eq!(expired.marker(), retained.marker());
        assert!(matches!(expired.details(), OutcomeDetails::Expired));
        assert!(execute(resolver, &handler, "expire").await.is_err());
        assert!(
            cold.requests.lock().unwrap().is_empty(),
            "lifecycle withdrawal precedes all detail reads"
        );
        assert_eq!(handler.calls.load(Ordering::SeqCst), 2);
        assert!(matches!(
            completed(&store, "unrelated").await?.details(),
            OutcomeDetails::Available(_)
        ));
    }
    Ok(())
}

fn proposal(id: &str, rejection: &str) -> EventTransaction {
    let metadata = metadata(id).with_default_outcome_evidence::<()>();
    EventTransaction::new(
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        Vec::new(),
    )
    .with_bounded_context(metadata.bounded_context().unwrap().clone())
    .with_completion(CommandCompletion::new(
        metadata.outcome_evidence().cloned(),
        Some(EncodedRejection {
            codec_id: "raw".to_owned(),
            codec_version: 1,
            bytes: rejection.as_bytes().to_vec(),
        }),
    ))
}
#[tokio::test]
async fn permanent_proposal_equality_remains_stricter_than_identity_after_expiry() -> TestResult {
    let store = InMemoryEventStore::new();
    let original = proposal("strict-proposal", "first-secret");
    store
        .begin_command()
        .await?
        .commit_outcome(original.clone())
        .await?;
    let retained = completed(&store, "strict-proposal").await?;
    store
        .expire_details(
            &key(&store, "strict-proposal"),
            retained.marker().revision(),
        )
        .await?;
    let replay = store
        .begin_command()
        .await?
        .commit_outcome(original)
        .await?;
    assert!(replay.is_exact_replay());
    assert!(matches!(
        replay.outcome().details(),
        OutcomeDetails::Expired
    ));
    let changed = store
        .begin_command()
        .await?
        .commit_outcome(proposal("strict-proposal", "different-secret"))
        .await
        .unwrap_err();
    assert_eq!(changed.kind(), EventStoreErrorKind::IdentityConflict);
    assert!(matches!(
        completed(&store, "strict-proposal").await?.details(),
        OutcomeDetails::Expired
    ));
    Ok(())
}
