use super::*;
use rostfrei_core::{HistoryCheckpoint, ReplayHistory};

/// Bloom membership is only a negative proof. Positive matches reload authoritative
/// history, including legacy operations whose receipts used a primary-derived subject.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Prefix {
    pub version: u64,
    pub sequence: u64,
    digest: String,
    identities: String,
    #[serde(skip)]
    bits: Vec<u8>,
}
impl Prefix {
    pub(super) fn contains(&self, operation: &OperationId) -> bool {
        membership(&self.bits, operation)
    }
}

fn positions(operation: &OperationId, bytes: usize) -> [usize; 16] {
    let digest = ContentFingerprint::digest(operation.as_str().as_bytes());
    let width = bytes.saturating_mul(8).max(1);
    let mut words = digest
        .as_bytes()
        .as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| u64::from_be_bytes(*chunk));
    let first = words.next().unwrap_or(0);
    let step = words.next().unwrap_or(1) | 1;
    let mut positions = [0; 16];
    for (index, position) in positions.iter_mut().enumerate() {
        let word = first.wrapping_add(step.wrapping_mul(u64::try_from(index).unwrap_or(0)));
        *position =
            usize::try_from(word.rem_euclid(u64::try_from(width).unwrap_or(u64::MAX))).unwrap_or(0);
    }
    positions
}
fn membership(bits: &[u8], operation: &OperationId) -> bool {
    positions(operation, bits.len()).into_iter().all(|bit| {
        bits.get(bit / 8)
            .is_none_or(|byte| byte & (1 << bit.rem_euclid(8)) != 0)
    })
}
fn add(bits: &mut [u8], operation: &OperationId) {
    for bit in positions(operation, bits.len()) {
        if let Some(byte) = bits.get_mut(bit / 8) {
            *byte |= 1 << bit.rem_euclid(8);
        }
    }
}

impl History {
    pub(super) fn version(&self) -> StreamVersion {
        self.events.last().map_or_else(
            || StreamVersion::new(self.prefix.as_ref().map_or(0, |prefix| prefix.version)),
            RecordedEvent::stream_version,
        )
    }
}

impl NatsEventStore {
    #[allow(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "bounded transaction evidence is validated in wire order"
    )]
    pub(super) async fn load_operation_history(
        &self,
        stream: &jetstream::stream::Stream,
        stream_id: &StreamId,
        participant: &TransactionParticipantWire,
        operation: &OperationId,
        event_ordinal: usize,
        event_count: usize,
        guard_ordinal: usize,
        batch_start: u64,
        layout: TransactionSubjectLayout<'_>,
    ) -> Result<Arc<History>, EventStoreError> {
        let subject = self.config.aggregate_subject(
            stream_id.aggregate_type().as_str(),
            stream_id.aggregate_id().as_str(),
        );
        let ordinal = if participant.event_count == 0 {
            event_count
                .checked_add(guard_ordinal)
                .ok_or_else(|| corrupt("guard ordinal overflow"))?
        } else {
            event_ordinal
        };
        let first_sequence = u64::try_from(ordinal)
            .ok()
            .and_then(|ordinal| batch_start.checked_add(ordinal))
            .ok_or_else(|| corrupt("operation sequence overflow"))?;
        let first = stream.get_raw_message(first_sequence).await.map_err(|e| {
            classify_required_raw_lookup_error(
                &e,
                "operation evidence is missing",
                "failed to read operation evidence",
            )
        })?;
        let expected_subject = if participant.event_count == 0 {
            layout.guard_subject(&self.config, operation.as_str(), guard_ordinal)
        } else {
            subject.clone()
        };
        if first.subject.as_str() != expected_subject || first.sequence != first_sequence {
            return Err(corrupt("operation evidence has wrong subject or position"));
        }
        let predecessor =
            required_single_header(&first.headers, "Nats-Expected-Last-Subject-Sequence")?
                .parse::<u64>()
                .map_err(|_| corrupt("invalid operation predecessor"))?;
        if predecessor >= batch_start
            || (participant.base_stream_version == 0) != (predecessor == 0)
        {
            return Err(corrupt("operation predecessor has invalid position"));
        }
        if predecessor > 0 {
            let previous = stream.get_raw_message(predecessor).await.map_err(|e| {
                classify_required_raw_lookup_error(
                    &e,
                    "operation predecessor is missing",
                    "failed to read operation predecessor",
                )
            })?;
            if previous.subject.as_str() != subject || previous.sequence != predecessor {
                return Err(corrupt("operation predecessor has wrong subject"));
            }
            let decoded = decode_event(
                &self.config,
                &subject,
                stream_id,
                None,
                predecessor,
                &previous.headers,
                &previous.payload,
            )?;
            if decoded.recorded.stream_version().value() != participant.base_stream_version
                || decoded.event_ordinal.checked_add(1) != Some(decoded.event_count)
            {
                return Err(corrupt(
                    "operation predecessor has wrong version or incomplete commit",
                ));
            }
        }
        let history = History {
            last_subject_stream_sequence: predecessor,
            prefix: (predecessor > 0).then(|| Prefix {
                version: participant.base_stream_version,
                sequence: predecessor,
                digest: String::new(),
                identities: STANDARD.encode([0; 256]),
                bits: vec![0; 256],
            }),
            ..History::default()
        };
        if participant.event_count == 0 {
            return Ok(Arc::new(history));
        }
        let mut builder = HistoryBuilder::from_history(&history);
        let mut last = first_sequence;
        for ordinal in 0..participant.event_count {
            let sequence = first_sequence
                .checked_add(u64::from(ordinal))
                .ok_or_else(|| corrupt("operation range overflow"))?;
            let message = if ordinal == 0 {
                first.clone()
            } else {
                stream.get_raw_message(sequence).await.map_err(|e| {
                    classify_required_raw_lookup_error(
                        &e,
                        "operation event is missing",
                        "failed to read operation event",
                    )
                })?
            };
            if message.sequence != sequence || message.subject.as_str() != subject {
                return Err(corrupt("operation event occupies the wrong slot"));
            }
            builder.push(decode_event(
                &self.config,
                &subject,
                stream_id,
                Some(predecessor),
                sequence,
                &message.headers,
                &message.payload,
            )?)?;
            last = sequence;
        }
        builder.finish(last).map(Arc::new)
    }

    pub(super) async fn snapshot_replay(
        &self,
        stream_id: &StreamId,
        after: Option<&HistoryCheckpoint>,
        through: Option<StreamVersion>,
    ) -> Result<(History, ReplayHistory), EventStoreError> {
        self.verify_authority().await?;
        let (history, read) = if self.snapshotting
            && !self.history_auditing
            && (after.is_some() || through.is_some())
        {
            Box::pin(self.load_checkpoint_tail(stream_id, after, through)).await?
        } else {
            let history = self.load_history(stream_id).await?;
            let read = history.events.len();
            (truncate(&history, through)?, read)
        };
        let base = after.map_or(0, |checkpoint| checkpoint.version);
        if let Some(checkpoint) = after {
            checkpoint.validate_for(self.authority.as_str(), stream_id)?;
            if base > history.version().value() {
                return Err(invalid("checkpoint is beyond committed head"));
            }
            // Full/audited fallback still validates the supplied native anchor.
            if history.prefix.is_none() {
                self.validate_checkpoint(stream_id, checkpoint).await?;
            }
        }
        let checkpoint = self.checkpoint(stream_id, &history).await?;
        self.verify_authority().await?;
        let events: Vec<_> = history
            .events
            .iter()
            .filter(|event| event.stream_version().value() > base)
            .cloned()
            .collect();
        Ok((
            history,
            ReplayHistory {
                base_version: StreamVersion::new(base),
                events,
                checkpoint,
                events_read: read,
            },
        ))
    }

    async fn checkpoint(
        &self,
        stream_id: &StreamId,
        history: &History,
    ) -> Result<Option<HistoryCheckpoint>, EventStoreError> {
        if history.version() == StreamVersion::ZERO {
            return Ok(None);
        }
        let capacity = history
            .commits
            .len()
            .saturating_mul(4)
            .clamp(256, 524_288)
            .next_power_of_two();
        let mut bits = if let Some(prefix) = &history.prefix {
            STANDARD
                .decode(&prefix.identities)
                .map_err(|_| corrupt("invalid checkpoint identity filter"))?
        } else {
            vec![0; capacity]
        };
        for commit in &history.commits {
            add(&mut bits, commit.batch.operation_id());
        }
        let subject = self.config.aggregate_subject(
            stream_id.aggregate_type().as_str(),
            stream_id.aggregate_id().as_str(),
        );
        let stream = self
            .context
            .get_stream(self.config.stream_name())
            .await
            .map_err(|e| unavailable(e.to_string()))?;
        let message = stream
            .get_raw_message(history.last_subject_stream_sequence)
            .await
            .map_err(|e| unavailable(e.to_string()))?;
        if message.subject.as_str() != subject {
            return Err(corrupt("checkpoint anchor has wrong subject"));
        }
        let prefix = Prefix {
            version: history.version().value(),
            sequence: history.last_subject_stream_sequence,
            digest: ContentFingerprint::digest(&message.payload).to_hex(),
            identities: STANDARD.encode(&bits),
            bits,
        };
        Ok(Some(HistoryCheckpoint {
            authority: self.authority.as_str().to_owned(),
            aggregate_type: stream_id.aggregate_type().as_str().to_owned(),
            aggregate_id: stream_id.aggregate_id().as_str().to_owned(),
            version: prefix.version,
            position: serde_json::to_vec(&prefix).map_err(|e| invalid(e.to_string()))?,
        }))
    }

    async fn validate_checkpoint(
        &self,
        stream_id: &StreamId,
        checkpoint: &HistoryCheckpoint,
    ) -> Result<Prefix, EventStoreError> {
        checkpoint.validate_for(self.authority.as_str(), stream_id)?;
        if self.stream_incarnation().await? != self.journal_incarnation {
            return Err(EventStoreError::new(
                EventStoreErrorKind::ConfigurationMismatch,
                "checkpoint authority was recreated",
            ));
        }
        let mut prefix: Prefix =
            serde_json::from_slice(&checkpoint.position).map_err(|e| invalid(e.to_string()))?;
        let bits = STANDARD
            .decode(&prefix.identities)
            .map_err(|_| invalid("invalid checkpoint identity filter"))?;
        if prefix.version != checkpoint.version
            || prefix.sequence == 0
            || bits.is_empty()
            || bits.len() > 524_288
        {
            return Err(invalid("invalid checkpoint boundary"));
        }
        prefix.bits = bits;
        let subject = self.config.aggregate_subject(
            stream_id.aggregate_type().as_str(),
            stream_id.aggregate_id().as_str(),
        );
        let stream = self
            .context
            .get_stream(self.config.stream_name())
            .await
            .map_err(|e| unavailable(e.to_string()))?;
        let message = stream.get_raw_message(prefix.sequence).await.map_err(|e| {
            classify_required_raw_lookup_error(
                &e,
                "checkpoint anchor is missing",
                "failed to read checkpoint anchor",
            )
        })?;
        if message.subject.as_str() != subject
            || ContentFingerprint::digest(&message.payload).to_hex() != prefix.digest
        {
            return Err(invalid("checkpoint anchor changed"));
        }
        let event = decode_event(
            &self.config,
            &subject,
            stream_id,
            None,
            prefix.sequence,
            &message.headers,
            &message.payload,
        )?;
        if event.recorded.stream_version().value() != prefix.version
            || event.event_ordinal.checked_add(1) != Some(event.event_count)
        {
            return Err(invalid("checkpoint must end at a complete commit"));
        }
        Ok(prefix)
    }

    async fn load_checkpoint_tail(
        &self,
        stream_id: &StreamId,
        checkpoint: Option<&HistoryCheckpoint>,
        through: Option<StreamVersion>,
    ) -> Result<(History, usize), EventStoreError> {
        let prefix = match checkpoint {
            Some(checkpoint) => Some(self.validate_checkpoint(stream_id, checkpoint).await?),
            None => None,
        };
        let base_version = prefix.as_ref().map_or(0, |prefix| prefix.version);
        let base_sequence = prefix.as_ref().map_or(0, |prefix| prefix.sequence);
        let subject = self.config.aggregate_subject(
            stream_id.aggregate_type().as_str(),
            stream_id.aggregate_id().as_str(),
        );
        let stream = self
            .context
            .get_stream(self.config.stream_name())
            .await
            .map_err(|e| unavailable(e.to_string()))?;
        let head = match stream.get_last_raw_message_by_subject(&subject).await {
            Ok(head) => head,
            Err(error)
                if error.kind() == LastRawMessageErrorKind::NoMessageFound && prefix.is_none() =>
            {
                if through.is_some_and(|target| target > StreamVersion::ZERO) {
                    return Err(invalid(
                        "snapshot/replay target is beyond the committed head",
                    ));
                }
                return Ok((History::default(), 0));
            }
            Err(error) => return Err(unavailable(error.to_string())),
        };
        let head_event = decode_event(
            &self.config,
            &subject,
            stream_id,
            None,
            head.sequence,
            &head.headers,
            &head.payload,
        )?;
        let target = through.unwrap_or_else(|| head_event.recorded.stream_version());
        if head.sequence < base_sequence
            || (head_event.recorded.stream_version().value() == base_version
                && head.sequence != base_sequence)
        {
            return Err(corrupt("head is inconsistent with checkpoint"));
        }
        if target > head_event.recorded.stream_version() || target.value() < base_version {
            return Err(invalid(
                "replay target is outside the committed checkpoint/head range",
            ));
        }
        let history = History {
            last_subject_stream_sequence: base_sequence,
            prefix,
            ..History::default()
        };
        if target.value() == base_version {
            return Ok((history, 0));
        }
        let start = base_sequence
            .checked_add(1)
            .ok_or_else(|| corrupt("native sequence overflow"))?;
        let mut reader =
            HistoryReader::starting_at(&self.context, &stream, &subject, start).await?;
        let result = self
            .read_checkpoint_tail(
                &mut reader,
                stream_id,
                &subject,
                &history,
                target,
                head.sequence,
            )
            .await;
        let cleanup = reader.close().await;
        let result = result?;
        cleanup?;
        if let Some(history) = result {
            let read = history.events.len();
            Ok((history, read))
        } else {
            let history = self.load_history(stream_id).await?;
            let read = history.events.len();
            Ok((truncate(&history, Some(target))?, read))
        }
    }

    async fn read_checkpoint_tail(
        &self,
        reader: &mut HistoryReader,
        stream_id: &StreamId,
        subject: &str,
        previous: &History,
        target: StreamVersion,
        cutoff: u64,
    ) -> Result<Option<History>, EventStoreError> {
        let mut builder = HistoryBuilder::from_history(previous);
        let mut last_commit = previous.last_subject_stream_sequence;
        let mut last_seen = last_commit;
        let empty_headers = HeaderMap::new();
        while builder.current_version < target {
            let message = reader
                .next()
                .await?
                .ok_or_else(|| corrupt("checkpoint tail ended before captured head"))?;
            let sequence = message
                .info()
                .map_err(|e| corrupt(e.to_string()))?
                .stream_sequence;
            if message.subject.as_str() != subject || sequence <= last_seen || sequence > cutoff {
                return Err(corrupt("invalid checkpoint tail position"));
            }
            let event = decode_event(
                &self.config,
                subject,
                stream_id,
                Some(last_commit),
                sequence,
                message.headers.as_ref().unwrap_or(&empty_headers),
                &message.payload,
            )?;
            if event.event_ordinal == 0
                && previous
                    .prefix
                    .as_ref()
                    .is_some_and(|prefix| prefix.contains(&event.operation_id))
            {
                // Confirm possible cross-boundary identity collisions against original history.
                return Ok(None);
            }
            if event.event_ordinal.checked_add(1) == Some(event.event_count) {
                last_commit = sequence;
            }
            builder.push(event)?;
            last_seen = sequence;
        }
        if builder.pending.is_some() {
            return Err(invalid(
                "snapshot/replay target must be the last event of a complete commit",
            ));
        }
        builder.finish(last_seen).map(Some)
    }
}

fn truncate(history: &History, target: Option<StreamVersion>) -> Result<History, EventStoreError> {
    let target = target.unwrap_or_else(|| history.version());
    if target > history.version() {
        return Err(invalid(
            "snapshot/replay target is beyond the committed head",
        ));
    }
    if target == StreamVersion::ZERO {
        return Ok(History::default());
    }
    let mut result = History::default();
    for commit in &history.commits {
        let last = commit
            .events
            .last()
            .ok_or_else(|| corrupt("empty stored commit"))?;
        if last.stream_version() > target {
            break;
        }
        result.events.extend(commit.events.iter().cloned());
        result
            .global_stream_sequences
            .extend(commit.global_stream_sequences.iter().copied());
        result.commits.push(commit.clone());
    }
    if result.version() != target {
        return Err(invalid(
            "snapshot/replay target must be the last event of a complete commit",
        ));
    }
    result.last_subject_stream_sequence = result
        .global_stream_sequences
        .last()
        .copied()
        .ok_or_else(|| corrupt("missing target native position"))?;
    Ok(result)
}
