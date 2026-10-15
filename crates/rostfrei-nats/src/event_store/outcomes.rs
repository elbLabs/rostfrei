//! Coordinated native outcome authority: permanent journal plus terminal last-value state.
use super::*;
use rostfrei_core::{
    CommandAuthority, CommandCommitSession, CompletedCommit, CompletedOutcome,
    EncodedOutcomeDetails, ExecutionStore, OutcomeDetails, OutcomeExpiry, OutcomeKey,
    OutcomeLifecycle, OutcomeLifecycleError, OutcomeLookup, OutcomeMarker, OutcomeReader,
    OutcomeRevision, ProposalDigest, ReclamationStatus,
};

pub(super) const JOURNAL_BINDING: &str = "rostfrei.split.journal-incarnation";
pub(super) const STATE_BINDING: &str = "rostfrei.split.state-incarnation";

#[derive(Clone)]
pub(super) struct SplitAuthority {
    state_incarnation: String,
}

fn configuration(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::ConfigurationMismatch, message)
}

pub(super) fn authority(
    config: &NatsEventStoreConfig,
    journal: &str,
    split: Option<&SplitAuthority>,
) -> Result<CommandAuthority, EventStoreError> {
    let mut hash = Sha256::new();
    for part in [
        config.stream_name(),
        config.subject_prefix(),
        config.completed_outcome_epoch().unwrap_or("legacy"),
        journal,
        split.map_or("inline", |s| s.state_incarnation.as_str()),
    ] {
        hash.update(
            u64::try_from(part.len())
                .map_err(|_| invalid("authority part length overflow"))?
                .to_be_bytes(),
        );
        hash.update(part.as_bytes());
    }
    CommandAuthority::new(format!("nats:{}", encode_lower_hex(hash.finalize())))
        .map_err(|e| invalid(e.to_string()))
}

pub(super) async fn connect_split(
    context: &jetstream::Context,
    config: &NatsEventStoreConfig,
    journal: &jetstream::stream::Stream,
) -> Result<Option<SplitAuthority>, EventStoreError> {
    if !config.split_outcomes() {
        return Ok(None);
    }
    let state = context
        .get_stream(config.outcome_state_stream_name())
        .await
        .map_err(|e| {
            if is_stream_not_found(&e) {
                configuration("outcome state authority is missing")
            } else {
                unavailable(e.to_string())
            }
        })?;
    let mut expected = config.outcome_state_config();
    if let Some(binding) = state.cached_info().config.metadata.get(JOURNAL_BINDING) {
        expected
            .metadata
            .insert(JOURNAL_BINDING.to_owned(), binding.clone());
    }
    let mismatches = stream_config_mismatches(&expected, &state.cached_info().config);
    if !mismatches.is_empty() {
        return Err(configuration(format!(
            "outcome state configuration differs: {}",
            mismatches.join(", ")
        )));
    }
    let journal_id = journal.cached_info().created.to_string();
    let state_id = state.cached_info().created.to_string();
    if journal.cached_info().config.metadata.get(STATE_BINDING) != Some(&state_id)
        || state.cached_info().config.metadata.get(JOURNAL_BINDING) != Some(&journal_id)
    {
        return Err(configuration(
            "journal/state incarnation binding differs; authority cannot be reopened",
        ));
    }
    Ok(Some(SplitAuthority {
        state_incarnation: state_id,
    }))
}

pub(super) async fn provision_split(
    context: &jetstream::Context,
    config: &NatsEventStoreConfig,
) -> Result<(), EventStoreError> {
    match context.get_stream(config.stream_name()).await {
        Ok(journal) => {
            verify_stream_config(&config.stream_config(), &journal.cached_info().config)?;
            connect_split(context, config, &journal).await?;
            return Ok(());
        }
        Err(e) if is_stream_not_found(&e) => {}
        Err(e) => return Err(unavailable(e.to_string())),
    }
    match context.get_stream(config.outcome_state_stream_name()).await {
        Ok(_) => {
            return Err(configuration(
                "orphan state authority forbids journal recreation",
            ));
        }
        Err(e) if is_stream_not_found(&e) => {}
        Err(e) => return Err(unavailable(e.to_string())),
    }
    // A partial provision stays fenced; only a completely cross-bound pair is usable.
    let state = context
        .create_stream(config.outcome_state_config())
        .await
        .map_err(|e| unavailable(e.to_string()))?;
    let mut journal_config = config.stream_config();
    journal_config.metadata.insert(
        STATE_BINDING.to_owned(),
        state.cached_info().created.to_string(),
    );
    let journal = context
        .create_stream(journal_config)
        .await
        .map_err(|e| unavailable(e.to_string()))?;
    let mut state_config = config.outcome_state_config();
    state_config.metadata.insert(
        JOURNAL_BINDING.to_owned(),
        journal.cached_info().created.to_string(),
    );
    context
        .update_stream(state_config)
        .await
        .map_err(|e| unavailable(e.to_string()))?;
    connect_split(context, config, &journal).await?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateEnvelope {
    version: u16,
    binding: String,
    state: StateValue,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum StateValue {
    Active { bytes: String },
    Expired { previous: u64, revision: String },
}
struct ObservedState {
    sequence: u64,
    value: StateValue,
}

impl NatsEventStore {
    pub(super) async fn acceptance_receipt(
        &self,
        operation: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        let Some(stored) = self
            .load_transaction_receipt_materialized(operation, false)
            .await?
        else {
            return Ok(None);
        };
        if stored
            .marker
            .as_ref()
            .is_some_and(|m| m.decision() == rostfrei_core::OutcomeDecision::Rejected)
        {
            return Err(identity_conflict(
                "operation has a completed rejection, not acceptance",
            ));
        }
        stored.receipt.require_acceptance().map(Some)
    }
    pub fn detail_reader(&self) -> Arc<dyn rostfrei_core::OutcomeDetailReader> {
        Arc::new(NativeDetailReader(self.clone()))
    }
    #[must_use]
    pub fn with_detail_readers(
        mut self,
        hot: Arc<dyn rostfrei_core::OutcomeDetailReader>,
        cold: Option<Arc<dyn rostfrei_core::OutcomeDetailReader>>,
    ) -> Self {
        self.detail_readers = Some((hot, cold));
        self
    }
    pub const fn authority(&self) -> &CommandAuthority {
        &self.authority
    }
    pub(super) fn outcome_key(&self, operation: &OperationId) -> OutcomeKey {
        OutcomeKey::new(
            self.authority.clone(),
            self.config.bounded_context().clone(),
            operation.clone(),
        )
    }
    async fn verify_journal(&self) -> Result<jetstream::stream::Stream, EventStoreError> {
        let stream = self
            .context
            .get_stream(self.config.stream_name())
            .await
            .map_err(|e| {
                if is_stream_not_found(&e) {
                    configuration("journal authority is missing")
                } else {
                    unavailable(e.to_string())
                }
            })?;
        verify_stream_config(&self.config.stream_config(), &stream.cached_info().config)?;
        if stream.cached_info().created.to_string() != self.journal_incarnation {
            return Err(configuration("journal authority was recreated"));
        }
        Ok(stream)
    }
    pub(super) async fn verify_authority(&self) -> Result<(), EventStoreError> {
        let journal = self.verify_journal().await?;
        let current = connect_split(&self.context, &self.config, &journal).await?;
        if current.as_ref().map(|s| &s.state_incarnation)
            != self.split.as_ref().map(|s| &s.state_incarnation)
        {
            return Err(configuration("outcome state authority was recreated"));
        }
        Ok(())
    }
    fn validate_key(&self, key: &OutcomeKey) -> Result<(), EventStoreError> {
        if key.authority() != &self.authority
            || key.bounded_context() != self.config.bounded_context()
        {
            return Err(configuration(
                "outcome key belongs to another configured authority",
            ));
        }
        Ok(())
    }
    async fn read_state(&self, marker: &OutcomeMarker) -> Result<ObservedState, EventStoreError> {
        self.verify_authority().await?;
        let manifest = marker.detail_reference().encode()?;
        let subject = self.config.outcome_state_subject(&manifest);
        let stream = self
            .context
            .get_stream(self.config.outcome_state_stream_name())
            .await
            .map_err(|e| unavailable(e.to_string()))?;
        let message = stream
            .get_last_raw_message_by_subject(&subject)
            .await
            .map_err(|e| match e.kind() {
                LastRawMessageErrorKind::NoMessageFound => {
                    corrupt("completed outcome state/lifecycle evidence is missing")
                }
                _ => unavailable(format!("outcome state read failed: {e}")),
            })?;
        if message.subject.as_str() != subject
            || message.sequence == 0
            || required_single_header(&message.headers, "Nats-Expected-Stream")?
                != self.config.outcome_state_stream_name()
            || optional_single_header(&message.headers, "Nats-Msg-Id")?.is_some()
            || optional_single_header(&message.headers, "Nats-Rollup")?.is_some()
            || optional_single_header(&message.headers, "KV-Operation")?.is_some()
        {
            return Err(corrupt("invalid outcome state coordinates or headers"));
        }
        let envelope: StateEnvelope =
            serde_json::from_slice(&message.payload).map_err(|e| corrupt(e.to_string()))?;
        if envelope.version != 1 || envelope.binding != STANDARD.encode(&manifest) {
            return Err(corrupt("outcome state format/reference binding mismatch"));
        }
        let expected =
            required_single_header(&message.headers, "Nats-Expected-Last-Subject-Sequence")?;
        match &envelope.state {
            StateValue::Active { bytes } => {
                if expected != "0" {
                    return Err(corrupt("Active was not created with raw CAS0"));
                }
                let bytes = STANDARD.decode(bytes).map_err(|e| corrupt(e.to_string()))?;
                let details = EncodedOutcomeDetails::decode(marker.detail_reference(), &bytes)?;
                if details.detail_reference() != marker.detail_reference() {
                    return Err(corrupt("detail reference differs"));
                }
            }
            StateValue::Expired { previous, revision } => {
                if revision != &marker.revision().to_hex() {
                    return Err(corrupt("expired state belongs to another winning revision"));
                }
                if *previous == 0
                    || *previous >= message.sequence
                    || expected != previous.to_string()
                {
                    return Err(corrupt("expired state has invalid transition evidence"));
                }
            }
        }
        Ok(ObservedState {
            sequence: message.sequence,
            value: envelope.state,
        })
    }
    async fn publish_state(
        &self,
        marker: &OutcomeMarker,
        value: StateValue,
        expected: u64,
    ) -> Result<u64, EventStoreError> {
        self.verify_authority().await?;
        let manifest = marker.detail_reference().encode()?;
        let payload = serde_json::to_vec(&StateEnvelope {
            version: 1,
            binding: STANDARD.encode(&manifest),
            state: value,
        })
        .map_err(|e| invalid(e.to_string()))?;
        if payload.len() > self.config.max_event_bytes() {
            return Err(invalid("outcome state exceeds configured message limit"));
        }
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json");
        headers.insert(
            NATS_EXPECTED_STREAM,
            self.config.outcome_state_stream_name(),
        );
        headers.insert(NATS_EXPECTED_LAST_SUBJECT_SEQUENCE, expected.to_string());
        let reply = self
            .context
            .client()
            .send_request(
                self.config.outcome_state_subject(&manifest),
                Request::new()
                    .headers(headers)
                    .payload(payload.into())
                    .timeout(Some(self.config.puback_timeout())),
            )
            .await
            .map_err(|e| unavailable(format!("state CAS acknowledgement unknown: {e}")))?;
        let ack = decode_atomic_publish_response(&reply.payload).map_err(|e| match e {
            AtomicBatchPublishError::Expectation => conflict("state CAS did not match"),
            AtomicBatchPublishError::Store(e) => e,
        })?;
        if ack.stream != self.config.outcome_state_stream_name()
            || ack.sequence == 0
            || ack.sequence <= expected
            || ack.batch.is_some()
        {
            return Err(corrupt("invalid state CAS PubAck"));
        }
        Ok(ack.sequence)
    }
    pub(super) async fn prepare_details(
        &self,
        marker: &OutcomeMarker,
        bytes: &[u8],
    ) -> Result<(), EventStoreError> {
        let result = self
            .publish_state(
                marker,
                StateValue::Active {
                    bytes: STANDARD.encode(bytes),
                },
                0,
            )
            .await;
        match self.read_state(marker).await {
            Ok(ObservedState {
                sequence,
                value: StateValue::Active { bytes: actual },
            }) => {
                if STANDARD
                    .decode(actual)
                    .map_err(|e| corrupt(e.to_string()))?
                    != bytes
                    || result.as_ref().is_ok_and(|ack| *ack != sequence)
                {
                    return Err(corrupt("prepared state differs from exact candidate"));
                }
                Ok(())
            }
            Ok(_) => Err(EventStoreError::new(
                EventStoreErrorKind::CompletedDetailsExpired,
                "candidate details are permanently expired",
            )),
            Err(error) => Err(result.err().unwrap_or(error)),
        }
    }
    async fn resolve_marker(
        &self,
        marker: OutcomeMarker,
    ) -> Result<CompletedOutcome, EventStoreError> {
        let observed = self
            .read_state(&marker)
            .await
            .map_err(EventStoreError::in_outcome_recovery)?;
        let details = match observed.value {
            StateValue::Active { bytes } => {
                if let Some((hot, cold)) = &self.detail_readers {
                    let details = rostfrei_core::resolve_details(
                        marker.detail_reference(),
                        hot.as_ref(),
                        cold.as_deref(),
                    )
                    .await
                    .map_err(EventStoreError::in_outcome_recovery)?;
                    if let OutcomeDetails::Available(value) = &details {
                        validate_recovered(&marker, value)
                            .map_err(EventStoreError::in_outcome_recovery)?;
                    }
                    details
                } else {
                    let details = EncodedOutcomeDetails::decode(
                        marker.detail_reference(),
                        &STANDARD.decode(bytes).map_err(|e| corrupt(e.to_string()))?,
                    )?;
                    validate_recovered(&marker, &details)
                        .map_err(EventStoreError::in_outcome_recovery)?;
                    OutcomeDetails::Available(details)
                }
            }
            StateValue::Expired { .. } => OutcomeDetails::Expired,
        };
        CompletedOutcome::new(marker, details)
            .map_err(|e| corrupt(e.to_string()).in_outcome_recovery())
    }
    pub(super) async fn matches_stored(
        &self,
        transaction: &EventTransaction,
        receipt: &TransactionReceipt,
    ) -> Result<bool, EventStoreError> {
        if !self.config.split_outcomes() {
            return Ok(transaction_matches_receipt(transaction, receipt));
        }
        let stored = self
            .load_transaction_receipt_materialized(transaction.operation_id(), true)
            .await?
            .ok_or_else(|| corrupt("winner missing"))?;
        if let Some(marker) = stored.marker {
            Ok(marker.proposal_digest() == ProposalDigest::of(transaction)?)
        } else {
            Ok(transaction_matches_receipt(transaction, receipt))
        }
    }
    pub(super) async fn resolve_identity_after_conflict(
        &self,
        transaction: &EventTransaction,
        receipt: Option<TransactionReceipt>,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        if !self.config.split_outcomes() {
            return resolve_transaction_identity_after_participant_conflict(transaction, receipt);
        }
        if let Some(receipt) = receipt
            && self.matches_stored(transaction, &receipt).await?
        {
            return Ok(TransactionAppendOutcome::ExactReplay(receipt));
        }
        Err(identity_conflict(
            "operation identity was already used with another proposal",
        ))
    }
    async fn commit_native(
        &self,
        proposal: EventTransaction,
        histories: LoadedHistories,
    ) -> Result<CompletedCommit, EventStoreError> {
        if !self.config.split_outcomes() {
            return Err(configuration(
                "native commands require a fresh fenced split authority; inline outcomes are read-only",
            ));
        }
        rostfrei_core::validate_completed_transaction(&proposal)?;
        self.validate_completed_scope(&proposal)?;
        self.require_completed_outcome_writer().await?;
        self.verify_authority().await?;
        let operation = proposal.operation_id().clone();
        let result = self
            .append_transaction_with_histories(proposal, histories)
            .await?;
        let OutcomeLookup::Completed(outcome) = self
            .lookup_outcome(&self.outcome_key(&operation))
            .await
            .map_err(EventStoreError::in_outcome_recovery)?
        else {
            return Err(corrupt("durable outcome disappeared").in_outcome_recovery());
        };
        Ok(match result {
            TransactionAppendOutcome::Appended(_) => CompletedCommit::Appended(outcome),
            TransactionAppendOutcome::ExactReplay(_) => CompletedCommit::ExactReplay(outcome),
        })
    }
    pub(super) async fn inline_lookup(
        &self,
        operation: &OperationId,
    ) -> Result<Option<rostfrei_core::InlineCompletedOutcome>, EventStoreError> {
        match self.lookup_outcome(&self.outcome_key(operation)).await? {
            OutcomeLookup::Missing => Ok(None),
            OutcomeLookup::Completed(outcome) => {
                Ok(Some(outcome.to_inline().map_err(recovery_error)?))
            }
        }
    }
    pub(super) async fn commit_inline(
        &self,
        proposal: EventTransaction,
        histories: LoadedHistories,
    ) -> Result<rostfrei_core::CompletedAppendOutcome, EventStoreError> {
        let result = self.commit_native(proposal, histories).await?;
        let inline = result.outcome().to_inline().map_err(recovery_error)?;
        Ok(if result.is_exact_replay() {
            rostfrei_core::CompletedAppendOutcome::ExactReplay(inline)
        } else {
            rostfrei_core::CompletedAppendOutcome::Appended(inline)
        })
    }
}

#[allow(clippy::needless_pass_by_value, reason = "owned map_err adapter")]
fn recovery_error(error: rostfrei_core::OutcomeRecoveryError) -> EventStoreError {
    EventStoreError::new(
        match error {
            rostfrei_core::OutcomeRecoveryError::Expired => {
                EventStoreErrorKind::CompletedDetailsExpired
            }
            rostfrei_core::OutcomeRecoveryError::IntegrityMismatch => {
                EventStoreErrorKind::CorruptHistory
            }
            _ => EventStoreErrorKind::CompletedDetailsUnavailable,
        },
        error.to_string(),
    )
    .in_outcome_recovery()
}

#[async_trait]
impl OutcomeReader for NatsEventStore {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        self.validate_key(key)?;
        self.verify_journal().await?;
        let Some(stored) = self
            .load_transaction_receipt_materialized(key.operation_id(), false)
            .await
            .map_err(EventStoreError::in_outcome_recovery)?
        else {
            if !self.config.split_outcomes() || self.config.read_only_outcomes() {
                return Err(configuration(
                    "missing outcome in read-only authority is not permission to execute",
                ));
            }
            // Absence permits Execute authorization only in the intact bound pair.
            // Completed reads verify the same pair in read_state, with recovery origin.
            self.verify_authority().await?;
            return Ok(OutcomeLookup::Missing);
        };
        if let Some(marker) = stored.marker {
            return self
                .resolve_marker(marker)
                .await
                .map(OutcomeLookup::Completed);
        }
        let mut receipt = stored.receipt;
        if receipt.completion().is_none() {
            return Err(
                configuration("legacy acceptance authority is unaudited").in_outcome_recovery()
            );
        }
        if let Some(scope) = self.config.legacy_security_scope() {
            if receipt
                .security_scope()
                .is_some_and(|stored| stored != scope)
            {
                return Err(
                    configuration("legacy scope mapping contradicts retained scope")
                        .in_outcome_recovery(),
                );
            }
            receipt = receipt.with_security_scope(scope.clone());
        } else if receipt.security_scope().is_some() {
            return Err(configuration(
                "policy-enabled legacy replay requires an explicit fixed scope mapping",
            )
            .in_outcome_recovery());
        }
        let proposal =
            proposal_from_receipt(&receipt).map_err(EventStoreError::in_outcome_recovery)?;
        let (marker, bytes) = OutcomeMarker::prepare(self.authority.clone(), &proposal, receipt)
            .map_err(EventStoreError::in_outcome_recovery)?;
        let details = EncodedOutcomeDetails::decode(marker.detail_reference(), &bytes)
            .map_err(EventStoreError::in_outcome_recovery)?;
        CompletedOutcome::new(marker, OutcomeDetails::Available(details))
            .map(OutcomeLookup::Completed)
            .map_err(recovery_error)
    }
}

#[async_trait]
impl ExecutionStore for NatsEventStore {
    fn authority(&self) -> &CommandAuthority {
        &self.authority
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        if !self.config.split_outcomes() {
            return Err(configuration(
                "native commands require a fresh fenced split authority",
            ));
        }
        self.require_completed_outcome_writer().await?;
        self.verify_authority().await?;
        Ok(Box::new(NatsAppendSession {
            store: self,
            histories: tokio::sync::Mutex::new(LoadedHistories::default()),
            incarnation: self.journal_incarnation.clone(),
        }))
    }
}
#[async_trait]
impl CommandCommitSession for NatsAppendSession<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        proposal: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        self.validate_incarnation().await?;
        self.store
            .commit_native(proposal, self.histories.into_inner())
            .await
    }
}

#[async_trait]
impl OutcomeLifecycle for NatsEventStore {
    async fn expire_details(
        &self,
        key: &OutcomeKey,
        expected: &OutcomeRevision,
    ) -> Result<OutcomeExpiry, OutcomeLifecycleError> {
        self.validate_key(key)?;
        if !self.config.split_outcomes()
            || !self.config.explicit_detail_expiry()
            || self.config.read_only_outcomes()
        {
            return Err(OutcomeLifecycleError::Unsupported);
        }
        self.verify_authority().await?;
        let stored = self
            .load_transaction_receipt_materialized(key.operation_id(), false)
            .await?
            .ok_or(OutcomeLifecycleError::Missing)?;
        let marker = stored.marker.ok_or(OutcomeLifecycleError::Unsupported)?;
        if marker.revision() != expected {
            return Err(OutcomeLifecycleError::RevisionMismatch);
        }
        let state = self.read_state(&marker).await?;
        let previous = match state.value {
            StateValue::Expired { previous, .. } => previous,
            StateValue::Active { bytes } => {
                let details = EncodedOutcomeDetails::decode(
                    marker.detail_reference(),
                    &STANDARD.decode(bytes).map_err(|e| corrupt(e.to_string()))?,
                )?;
                validate_recovered(&marker, &details)?;
                let result = self
                    .publish_state(
                        &marker,
                        StateValue::Expired {
                            previous: state.sequence,
                            revision: expected.to_hex(),
                        },
                        state.sequence,
                    )
                    .await;
                match self.read_state(&marker).await {
                    Ok(ObservedState {
                        sequence,
                        value: StateValue::Expired { previous, .. },
                    }) => {
                        if result.as_ref().is_ok_and(|ack| *ack != sequence) {
                            return Err(
                                corrupt("expiry PubAck differs from terminal sequence").into()
                            );
                        }
                        previous
                    }
                    Ok(_) => {
                        return Err(result
                            .err()
                            .unwrap_or_else(|| unavailable("expiry transition remains unconfirmed"))
                            .into());
                    }
                    Err(error) => return Err(result.err().unwrap_or(error).into()),
                }
            }
        };
        let reclamation = match self
            .context
            .get_stream(self.config.outcome_state_stream_name())
            .await
        {
            Ok(stream) => match stream.get_raw_message(previous).await {
                Err(e) if e.kind() == jetstream::stream::RawMessageErrorKind::NoMessageFound => {
                    ReclamationStatus::Reclaimed
                }
                _ => ReclamationStatus::Pending,
            },
            Err(_) => ReclamationStatus::Pending,
        };
        Ok(OutcomeExpiry::new(expected.clone(), reclamation))
    }
}

struct NativeDetailReader(NatsEventStore);
#[async_trait]
impl rostfrei_core::OutcomeDetailReader for NativeDetailReader {
    async fn get(
        &self,
        reference: &rostfrei_core::OutcomeDetailReference,
    ) -> Result<rostfrei_core::DetailRead, rostfrei_core::DetailReadError> {
        use rostfrei_core::{DetailRead, DetailReadError};
        let read = async {
            self.0.validate_key(reference.key())?;
            let stored = self
                .0
                .load_transaction_receipt_materialized(reference.key().operation_id(), true)
                .await?
                .ok_or_else(|| corrupt("detail winner missing"))?;
            let marker = stored
                .marker
                .ok_or_else(|| configuration("inline authority has no split reader"))?;
            if marker.detail_reference() != reference {
                return Err(corrupt("detail reference is not the winning reference"));
            }
            match self.0.read_state(&marker).await?.value {
                StateValue::Active { bytes } => Ok(DetailRead::Found(
                    STANDARD.decode(bytes).map_err(|e| corrupt(e.to_string()))?,
                )),
                StateValue::Expired { .. } => Ok(DetailRead::AbsentHere),
            }
        }
        .await;
        read.map_err(|e| match e.kind() {
            EventStoreErrorKind::Unavailable => DetailReadError::Unavailable,
            EventStoreErrorKind::ConfigurationMismatch => DetailReadError::ConfigurationMismatch,
            EventStoreErrorKind::DetailForbidden => DetailReadError::Forbidden,
            _ => DetailReadError::Corrupt,
        })
    }
}

pub(super) fn staged_receipt(
    proposal: &EventTransaction,
    staged: &[StagedTransactionParticipant],
) -> TransactionReceipt {
    let mut receipt = TransactionReceipt::new(
        proposal.operation_id().clone(),
        proposal.operation_fingerprint(),
        staged
            .iter()
            .map(|s| {
                TransactionStreamReceipt::new(
                    s.stream_id.clone(),
                    s.base_version,
                    s.recorded.clone(),
                )
            })
            .collect(),
    );
    if let Some(context) = proposal.bounded_context() {
        receipt = receipt.with_bounded_context(context.clone());
    }
    if let Some(id) = proposal.correlation_id() {
        receipt = receipt.with_correlation_id(id.clone());
    }
    if let Some(id) = proposal.causation_id() {
        receipt = receipt.with_causation_id(id.clone());
    }
    if let Some(scope) = proposal.security_scope() {
        receipt = receipt.with_security_scope(scope.clone());
    }
    receipt
}

pub(super) fn proposal_from_receipt(
    receipt: &TransactionReceipt,
) -> Result<EventTransaction, EventStoreError> {
    receipt.validate_integrity()?;
    let mut participants = Vec::new();
    for stream in receipt.streams() {
        let batch = if let Some(first) = stream.events().first() {
            let mut batch = EventBatch::new(
                first.commit_id().clone(),
                receipt.operation_id().clone(),
                receipt.operation_fingerprint(),
                stream
                    .events()
                    .iter()
                    .map(|e| {
                        NewEvent::new(
                            e.event_id().clone(),
                            e.event_type(),
                            e.schema_version(),
                            e.payload().to_vec(),
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| corrupt(e.to_string()))?,
            )
            .map_err(|e| corrupt(e.to_string()))?;
            if let Some(id) = receipt.correlation_id() {
                batch = batch.with_correlation_id(id.clone());
            }
            if let Some(id) = receipt.causation_id() {
                batch = batch.with_causation_id(id.clone());
            }
            Some(batch)
        } else {
            None
        };
        participants.push(TransactionParticipant::new(
            stream.stream_id().clone(),
            if stream.base_version() == StreamVersion::ZERO {
                ExpectedVersion::NoStream
            } else {
                ExpectedVersion::Exact(stream.base_version())
            },
            batch,
        ));
    }
    let mut proposal = EventTransaction::new(
        receipt.operation_id().clone(),
        receipt.operation_fingerprint(),
        participants,
    );
    if let Some(context) = receipt.bounded_context() {
        proposal = proposal.with_bounded_context(context.clone());
    }
    if let Some(id) = receipt.correlation_id() {
        proposal = proposal.with_correlation_id(id.clone());
    }
    if let Some(id) = receipt.causation_id() {
        proposal = proposal.with_causation_id(id.clone());
    }
    if let Some(scope) = receipt.security_scope() {
        proposal = proposal.with_security_scope(scope.clone());
    }
    if let Some(completion) = receipt.completion() {
        proposal = proposal.with_completion(completion.clone());
    }
    Ok(proposal)
}

fn validate_recovered(
    marker: &OutcomeMarker,
    details: &EncodedOutcomeDetails,
) -> Result<(), EventStoreError> {
    let mut receipt = marker.integrity_receipt().clone();
    if let Some(completion) = details.completion() {
        receipt = receipt.with_completion(completion.clone());
    }
    let proposal = proposal_from_receipt(&receipt)?;
    let (reconstructed, _) = OutcomeMarker::prepare(
        marker.detail_reference().key().authority().clone(),
        &proposal,
        receipt,
    )?;
    if reconstructed != *marker {
        return Err(corrupt(
            "recovered details/proposal/permanent manifest mismatch",
        ));
    }
    Ok(())
}
