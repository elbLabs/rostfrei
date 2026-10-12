use std::ops::RangeInclusive;

use async_nats::jetstream::{
    message::StreamMessage,
    stream::{LastRawMessageErrorKind, Stream},
};
use async_trait::async_trait;
use futures_util::future::try_join_all;

use super::{
    EventStoreError, History, HistoryBuilder, NatsEventStoreConfig, StreamId, corrupt,
    decode_event, unavailable,
};

const MAX_HISTORY_READ_RANGES: u64 = 8;
const MAX_WINDOW_SEQUENCES: u64 = 128;
const HISTORY_CHUNK_BYTES: usize = 256 * 1024;

struct Chunk {
    messages: Vec<StreamMessage>,
    progress: Progress,
}

enum Progress {
    Resume(u64),
    WindowEnd,
    Lookahead {
        sequence: u64,
        message: Option<StreamMessage>,
    },
    Exhausted,
}

#[async_trait]
pub(super) trait SubjectReader: Sync {
    async fn next(
        &self,
        subject: &str,
        sequence: u64,
    ) -> Result<Option<StreamMessage>, EventStoreError>;
}

#[async_trait]
impl SubjectReader for Stream {
    async fn next(
        &self,
        subject: &str,
        sequence: u64,
    ) -> Result<Option<StreamMessage>, EventStoreError> {
        match self
            .get_first_raw_message_by_subject(subject, sequence)
            .await
        {
            Ok(message) => Ok(Some(message)),
            Err(error) if error.kind() == LastRawMessageErrorKind::NoMessageFound => Ok(None),
            Err(error) => Err(unavailable(format!(
                "failed to read aggregate history: {error}"
            ))),
        }
    }
}

pub(super) async fn load_prefix(
    stream: &impl SubjectReader,
    config: &NatsEventStoreConfig,
    stream_id: &StreamId,
    subject: &str,
    last_sequence: u64,
    truncated_at_snapshot: bool,
) -> Result<History, EventStoreError> {
    load_prefix_with_parallelism(
        stream,
        config,
        stream_id,
        subject,
        last_sequence,
        truncated_at_snapshot,
        MAX_HISTORY_READ_RANGES,
    )
    .await
}

#[cfg(test)]
pub(super) async fn load_prefix_serial(
    stream: &impl SubjectReader,
    config: &NatsEventStoreConfig,
    stream_id: &StreamId,
    subject: &str,
    last_sequence: u64,
    truncated_at_snapshot: bool,
) -> Result<History, EventStoreError> {
    load_prefix_with_parallelism(
        stream,
        config,
        stream_id,
        subject,
        last_sequence,
        truncated_at_snapshot,
        1,
    )
    .await
}

async fn load_prefix_with_parallelism(
    stream: &impl SubjectReader,
    config: &NatsEventStoreConfig,
    stream_id: &StreamId,
    subject: &str,
    last_sequence: u64,
    truncated_at_snapshot: bool,
    parallelism: u64,
) -> Result<History, EventStoreError> {
    let mut fold = HistoryFold::default();
    if last_sequence == 0 {
        return fold.finish(last_sequence, truncated_at_snapshot);
    }
    // Locate the aggregate's first record before dividing its remaining span.
    // A newly created aggregate near the end of a busy store needs one lookup,
    // rather than a wave of identical lookahead responses over unrelated history.
    let Some(first) = stream.next(subject, 1).await? else {
        return fold.finish(last_sequence, truncated_at_snapshot);
    };
    if first.subject.as_str() != subject || first.sequence == 0 {
        return Err(corrupt(
            "aggregate history returned an invalid first message",
        ));
    }
    if first.sequence > last_sequence {
        return fold.finish(last_sequence, truncated_at_snapshot);
    }
    let mut cursor = first.sequence;
    let mut seed = Some(first);
    let mut effective_parallelism = parallelism;
    while cursor <= last_sequence {
        let ranges = sequence_ranges(cursor, last_sequence, effective_parallelism);
        let final_window = ranges
            .last()
            .map(|range| *range.end())
            .ok_or_else(|| corrupt("aggregate history has no read windows"))?;
        let mut reads = Vec::new();
        for range in &ranges {
            let retain_through = (*range.end() == final_window).then_some(last_sequence);
            reads.push(read_chunk(
                stream,
                subject,
                range.clone(),
                seed.take(),
                retain_through,
            ));
        }
        // Each window is small enough to finish concurrently for ordinary
        // events. Count and byte bounds also permit oversized historical records.
        // No tasks are spawned: cancellation drops all pending reads and results.
        let chunks = try_join_all(reads).await?;
        let mut progress = Progress::Exhausted;
        let mut window_end = cursor;
        let mut wave_events = 0_u64;
        for (range, mut chunk) in ranges.into_iter().zip(chunks) {
            window_end = *range.end();
            loop {
                for message in chunk.messages {
                    fold.push(config, stream_id, subject, &message)?;
                    wave_events = wave_events.saturating_add(1);
                }
                match chunk.progress {
                    Progress::Resume(next) => {
                        let retain_through = (window_end == final_window).then_some(last_sequence);
                        chunk =
                            read_chunk(stream, subject, next..=window_end, None, retain_through)
                                .await?;
                    }
                    completed => {
                        progress = completed;
                        break;
                    }
                }
            }
        }
        if window_end == last_sequence {
            break;
        }
        // Very sparse histories otherwise issue mostly duplicate lookahead
        // reads. After observing that shape, keep using indexed successor reads
        // but stop multiplying their broker work by the lane count.
        if wave_events > 0 && wave_events < effective_parallelism {
            effective_parallelism = 1;
        }
        // Only the final, fully consumed window can certify the next frontier.
        // Its subject-successor lookup proves that gaps contain no matching
        // messages, so sparse histories do not scan the entire global log.
        match progress {
            Progress::Lookahead { sequence, message } if sequence <= last_sequence => {
                cursor = sequence;
                seed = message;
            }
            Progress::WindowEnd => {
                cursor = window_end
                    .checked_add(1)
                    .ok_or_else(|| corrupt("JetStream sequence space overflowed"))?;
            }
            Progress::Resume(_) => return Err(corrupt("aggregate history window is incomplete")),
            Progress::Lookahead { .. } | Progress::Exhausted => break,
        }
    }
    fold.finish(last_sequence, truncated_at_snapshot)
}

fn sequence_ranges(
    first_sequence: u64,
    last_sequence: u64,
    parallelism: u64,
) -> Vec<RangeInclusive<u64>> {
    let width = last_sequence
        .saturating_sub(first_sequence)
        .saturating_add(1)
        .div_ceil(parallelism)
        .min(MAX_WINDOW_SEQUENCES);
    let mut ranges = Vec::new();
    let mut start = first_sequence;
    for _ in 0..parallelism {
        if start > last_sequence {
            break;
        }
        let end = start
            .saturating_add(width.saturating_sub(1))
            .min(last_sequence);
        ranges.push(start..=end);
        if end == last_sequence {
            break;
        }
        start = end.saturating_add(1);
    }
    ranges
}

async fn read_chunk(
    reader: &impl SubjectReader,
    subject: &str,
    range: RangeInclusive<u64>,
    mut seed: Option<StreamMessage>,
    retain_lookahead_through: Option<u64>,
) -> Result<Chunk, EventStoreError> {
    let mut cursor = *range.start();
    let mut messages = Vec::new();
    let mut bytes = 0_usize;
    let progress = loop {
        let response = match seed.take() {
            Some(message) => Some(message),
            None => reader.next(subject, cursor).await?,
        };
        let Some(message) = response else {
            break Progress::Exhausted;
        };
        if message.subject.as_str() != subject {
            return Err(corrupt("aggregate history contains the wrong subject"));
        }
        if message.sequence < cursor {
            return Err(corrupt(
                "aggregate history returned an invalid stream sequence",
            ));
        }
        if message.sequence > *range.end() {
            break Progress::Lookahead {
                sequence: message.sequence,
                message: retain_lookahead_through
                    .is_some_and(|limit| message.sequence <= limit)
                    .then_some(message),
            };
        }
        let sequence = message.sequence;
        bytes = bytes.saturating_add(message_bytes(&message));
        messages.push(message);
        if sequence == *range.end() {
            break Progress::WindowEnd;
        }
        cursor = sequence
            .checked_add(1)
            .ok_or_else(|| corrupt("JetStream sequence space overflowed"))?;
        if bytes >= HISTORY_CHUNK_BYTES {
            break Progress::Resume(cursor);
        }
    };
    Ok(Chunk { messages, progress })
}

fn message_bytes(message: &StreamMessage) -> usize {
    message.headers.iter().fold(
        message.payload.len().saturating_add(message.subject.len()),
        |total, (name, values)| {
            let name: &str = name.as_ref();
            values.iter().fold(total, |total, value| {
                total
                    .saturating_add(name.len())
                    .saturating_add(value.as_str().len())
                    .saturating_add(4)
            })
        },
    )
}

#[derive(Default)]
struct HistoryFold {
    history: HistoryBuilder,
    last_commit_sequence: u64,
    last_seen_sequence: u64,
}

impl HistoryFold {
    fn push(
        &mut self,
        config: &NatsEventStoreConfig,
        stream_id: &StreamId,
        subject: &str,
        message: &StreamMessage,
    ) -> Result<(), EventStoreError> {
        if message.sequence <= self.last_seen_sequence {
            return Err(corrupt(
                "aggregate history returned an invalid stream sequence",
            ));
        }
        let decoded = decode_event(
            config,
            subject,
            stream_id,
            Some(self.last_commit_sequence),
            message.sequence,
            &message.headers,
            &message.payload,
        )?;
        let next_ordinal = decoded
            .event_ordinal
            .checked_add(1)
            .ok_or_else(|| corrupt("stored event has invalid commit coordinates"))?;
        if next_ordinal == decoded.event_count {
            self.last_commit_sequence = message.sequence;
        }
        self.history.push(decoded)?;
        self.last_seen_sequence = message.sequence;
        Ok(())
    }

    fn finish(
        self,
        last_sequence: u64,
        truncated_at_snapshot: bool,
    ) -> Result<History, EventStoreError> {
        if !truncated_at_snapshot && self.last_seen_sequence != last_sequence {
            return Err(corrupt("aggregate history ended before its last message"));
        }
        if self.last_seen_sequence == 0 && truncated_at_snapshot {
            return Ok(History::default());
        }
        self.history.finish(self.last_seen_sequence)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use rostfrei_core::{AggregateId, AggregateType, EventStoreErrorKind};
    use rostfrei_messaging_core::ApplicationName;
    use tokio::sync::Barrier;

    use super::*;

    fn fixture() -> (NatsEventStoreConfig, StreamId) {
        let application = ApplicationName::new("history-read-tests").unwrap();
        let context = application.bounded_context("audit").unwrap();
        (
            NatsEventStoreConfig::for_bounded_context(&context).unwrap(),
            StreamId::new(
                AggregateType::new("counter").unwrap(),
                AggregateId::new("one").unwrap(),
            ),
        )
    }

    fn message(subject: &str, sequence: u64) -> StreamMessage {
        StreamMessage {
            subject: subject.to_owned().into(),
            sequence,
            headers: async_nats::HeaderMap::new(),
            payload: Vec::new().into(),
            time: time::OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn ranges_partition_the_cutoff_without_overflow() {
        for last in [0, 1, 7, 8, 9, 100, u64::MAX] {
            let ranges = sequence_ranges(1, last, MAX_HISTORY_READ_RANGES);
            assert!(ranges.len() <= usize::try_from(MAX_HISTORY_READ_RANGES).unwrap());
            if last == 0 {
                assert!(ranges.is_empty());
                continue;
            }
            assert_eq!(ranges.first().map(RangeInclusive::start), Some(&1));
            let window_end = last.min(1024);
            assert_eq!(ranges.last().map(RangeInclusive::end), Some(&window_end));
            let mut previous = 0_u64;
            for range in ranges {
                assert_eq!(previous.checked_add(1), Some(*range.start()));
                assert!(range.start() <= range.end());
                previous = *range.end();
            }
        }
        assert_eq!(
            sequence_ranges(u64::MAX, u64::MAX, MAX_HISTORY_READ_RANGES),
            vec![u64::MAX..=u64::MAX]
        );
    }

    struct BarrierReader(Barrier);

    #[async_trait]
    impl SubjectReader for BarrierReader {
        async fn next(
            &self,
            subject: &str,
            sequence: u64,
        ) -> Result<Option<StreamMessage>, EventStoreError> {
            if sequence == 1 {
                return Ok(Some(message(subject, sequence)));
            }
            self.0.wait().await;
            Ok(None)
        }
    }

    #[tokio::test]
    async fn independent_ranges_progress_concurrently() {
        let (config, id) = fixture();
        let reader = BarrierReader(Barrier::new(8));
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            load_prefix(&reader, &config, &id, "events", 64, true),
        )
        .await
        .unwrap();
        assert_eq!(
            result.err().unwrap().kind(),
            EventStoreErrorKind::CorruptHistory
        );
    }

    struct PendingReader(AtomicUsize);
    struct ActiveRead<'a>(&'a AtomicUsize);

    impl Drop for ActiveRead<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[async_trait]
    impl SubjectReader for PendingReader {
        async fn next(
            &self,
            subject: &str,
            sequence: u64,
        ) -> Result<Option<StreamMessage>, EventStoreError> {
            if sequence == 1 {
                return Ok(Some(message(subject, sequence)));
            }
            self.0.fetch_add(1, Ordering::SeqCst);
            let _active = ActiveRead(&self.0);
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn cancellation_drops_every_outstanding_range_read() {
        let (config, id) = fixture();
        let reader = PendingReader(AtomicUsize::new(0));
        let mut load = Box::pin(load_prefix(&reader, &config, &id, "events", 64, true));
        assert!(futures_util::poll!(load.as_mut()).is_pending());
        assert_eq!(reader.0.load(Ordering::SeqCst), 8);
        drop(load);
        assert_eq!(reader.0.load(Ordering::SeqCst), 0);
    }

    struct FailingReader;

    #[async_trait]
    impl SubjectReader for FailingReader {
        async fn next(
            &self,
            subject: &str,
            sequence: u64,
        ) -> Result<Option<StreamMessage>, EventStoreError> {
            if sequence == 1 {
                Ok(Some(message(subject, sequence)))
            } else if sequence == 17 {
                Err(unavailable("injected later-range failure"))
            } else {
                Ok(None)
            }
        }
    }

    #[tokio::test]
    async fn exhausted_windows_do_not_hide_a_read_failure() {
        let (config, id) = fixture();
        let error = load_prefix(&FailingReader, &config, &id, "events", 64, true)
            .await
            .err()
            .unwrap();
        assert_eq!(error.kind(), EventStoreErrorKind::Unavailable);
        assert_eq!(error.message(), "injected later-range failure");
    }

    struct FixedReader(StreamMessage);

    #[async_trait]
    impl SubjectReader for FixedReader {
        async fn next(
            &self,
            _subject: &str,
            _sequence: u64,
        ) -> Result<Option<StreamMessage>, EventStoreError> {
            Ok(Some(self.0.clone()))
        }
    }

    #[tokio::test]
    async fn malformed_range_coordinates_fail_before_exhaustion() {
        for response in [message("events", 9), message("wrong-subject", 21)] {
            let error = read_chunk(&FixedReader(response), "events", 10..=20, None, None)
                .await
                .err()
                .unwrap();
            assert_eq!(error.kind(), EventStoreErrorKind::CorruptHistory);
        }
        let chunk = read_chunk(
            &FixedReader(message("events", u64::MAX)),
            "events",
            u64::MAX..=u64::MAX,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(chunk.messages.first().unwrap().sequence, u64::MAX);
        assert!(matches!(chunk.progress, Progress::WindowEnd));
    }

    struct CountingReader(AtomicUsize);

    #[async_trait]
    impl SubjectReader for CountingReader {
        async fn next(
            &self,
            subject: &str,
            sequence: u64,
        ) -> Result<Option<StreamMessage>, EventStoreError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let mut result = message(subject, sequence);
            result.payload = vec![0; HISTORY_CHUNK_BYTES].into();
            Ok(Some(result))
        }
    }

    #[tokio::test]
    async fn oversized_messages_bound_a_chunk_and_preserve_the_resume_cursor() {
        let reader = CountingReader(AtomicUsize::new(0));
        let chunk = read_chunk(&reader, "events", 1..=100, None, None)
            .await
            .unwrap();
        assert_eq!(reader.0.load(Ordering::SeqCst), 1);
        assert_eq!(chunk.messages.len(), 1);
        assert!(matches!(chunk.progress, Progress::Resume(2)));
    }

    struct WaveReader {
        barrier: Barrier,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl SubjectReader for WaveReader {
        async fn next(
            &self,
            subject: &str,
            sequence: u64,
        ) -> Result<Option<StreamMessage>, EventStoreError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !sequence
                .saturating_sub(1)
                .is_multiple_of(MAX_WINDOW_SEQUENCES)
            {
                self.barrier.wait().await;
            }
            Ok(Some(message(subject, sequence)))
        }
    }

    #[tokio::test]
    async fn windows_sustain_concurrency_beyond_initial_prefetch() {
        let (config, id) = fixture();
        let reader = WaveReader {
            barrier: Barrier::new(8),
            calls: AtomicUsize::new(0),
        };
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            load_prefix(&reader, &config, &id, "events", 2048, false),
        )
        .await
        .unwrap();
        // The synthetic payload is invalid, but all eight 128-message windows
        // must finish before decoding. A tiny channel per whole-history range
        // would deadlock at the third barrier instead of making sustained progress.
        assert_eq!(
            result.err().unwrap().kind(),
            EventStoreErrorKind::CorruptHistory
        );
        assert_eq!(reader.calls.load(Ordering::SeqCst), 1024);
    }

    struct SparseReader {
        event: StreamMessage,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl SubjectReader for SparseReader {
        async fn next(
            &self,
            _subject: &str,
            sequence: u64,
        ) -> Result<Option<StreamMessage>, EventStoreError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok((sequence <= self.event.sequence).then(|| self.event.clone()))
        }
    }

    #[tokio::test]
    async fn sparse_histories_jump_gaps_and_reuse_lookahead_even_at_maximum_sequence() {
        use super::super::{
            ContentFingerprint, EventBatch, NewEvent, OperationId, StreamVersion, derive_commit_id,
            derive_event_id, encode_events, record_batch,
        };
        let (config, id) = fixture();
        let operation = OperationId::new("sparse").unwrap();
        let commit = derive_commit_id(&id, &operation);
        let batch = EventBatch::new(
            commit.clone(),
            operation,
            ContentFingerprint::digest("sparse"),
            vec![
                NewEvent::new(
                    derive_event_id(&commit, 0),
                    "incremented",
                    1,
                    b"{}".to_vec(),
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let recorded = record_batch(&id, StreamVersion::ZERO, &batch).unwrap();
        let payload = encode_events(&config, &id, &batch, recorded.events())
            .unwrap()
            .pop()
            .unwrap();
        let subject =
            config.aggregate_subject(id.aggregate_type().as_str(), id.aggregate_id().as_str());
        for sequence in [1, 1000, 1_000_000_000, u64::MAX] {
            let mut event = message(&subject, sequence);
            event.payload = payload.clone().into();
            for (name, value) in [
                ("Content-Type", "application/json"),
                ("Nats-Batch-Id", "sparse-batch"),
                ("Nats-Batch-Sequence", "1"),
                ("Nats-Expected-Stream", config.stream_name()),
                ("Nats-Expected-Last-Subject-Sequence", "0"),
                ("Nats-Batch-Commit", "1"),
            ] {
                event.headers.insert(name, value);
            }
            let reader = SparseReader {
                event,
                calls: AtomicUsize::new(0),
            };
            let history = load_prefix(&reader, &config, &id, &subject, sequence, false)
                .await
                .unwrap();
            assert_eq!(history.events, recorded.events());
            assert!(reader.calls.load(Ordering::SeqCst) <= 8);
        }
    }

    struct HistoryReader(Vec<StreamMessage>);

    #[async_trait]
    impl SubjectReader for HistoryReader {
        async fn next(
            &self,
            _subject: &str,
            sequence: u64,
        ) -> Result<Option<StreamMessage>, EventStoreError> {
            let index = self
                .0
                .partition_point(|message| message.sequence < sequence);
            Ok(self.0.get(index).cloned())
        }
    }

    fn commit_messages(
        config: &NatsEventStoreConfig,
        id: &StreamId,
        base_version: u64,
        first_sequence: u64,
        previous_sequence: u64,
        count: u32,
        payload_bytes: usize,
    ) -> Vec<StreamMessage> {
        use super::super::{
            ContentFingerprint, EventBatch, NewEvent, OperationId, StreamVersion, derive_commit_id,
            derive_event_id, encode_events, record_batch,
        };
        let operation = OperationId::new(format!("commit-{first_sequence}")).unwrap();
        let commit = derive_commit_id(id, &operation);
        let batch = EventBatch::new(
            commit.clone(),
            operation,
            ContentFingerprint::digest("history"),
            (0..count)
                .map(|ordinal| {
                    NewEvent::new(
                        derive_event_id(&commit, ordinal),
                        "incremented",
                        1,
                        vec![b'x'; payload_bytes],
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap();
        let recorded = record_batch(id, StreamVersion::new(base_version), &batch).unwrap();
        let subject =
            config.aggregate_subject(id.aggregate_type().as_str(), id.aggregate_id().as_str());
        encode_events(config, id, &batch, recorded.events())
            .unwrap()
            .into_iter()
            .enumerate()
            .map(|(index, payload)| {
                let offset = u64::try_from(index).unwrap();
                let mut event = message(&subject, first_sequence.checked_add(offset).unwrap());
                event.payload = payload.into();
                event.headers.insert("Content-Type", "application/json");
                event
                    .headers
                    .insert("Nats-Batch-Id", format!("batch-{first_sequence}"));
                event.headers.insert(
                    "Nats-Batch-Sequence",
                    offset.checked_add(1).unwrap().to_string(),
                );
                if index == 0 {
                    event
                        .headers
                        .insert("Nats-Expected-Stream", config.stream_name());
                    event.headers.insert(
                        "Nats-Expected-Last-Subject-Sequence",
                        previous_sequence.to_string(),
                    );
                }
                if offset.checked_add(1) == Some(u64::from(count)) {
                    event.headers.insert("Nats-Batch-Commit", "1");
                }
                event
            })
            .collect()
    }

    #[tokio::test]
    async fn wave_boundaries_preserve_pending_commits_and_reject_omissions() {
        let (config, id) = fixture();
        let subject =
            config.aggregate_subject(id.aggregate_type().as_str(), id.aggregate_id().as_str());
        let mut messages = Vec::new();
        for commit in 0_u64..12 {
            let base = commit.checked_mul(100).unwrap();
            messages.extend(commit_messages(
                &config,
                &id,
                base,
                base.checked_add(1).unwrap(),
                base,
                100,
                2,
            ));
        }
        let history = load_prefix(
            &HistoryReader(messages.clone()),
            &config,
            &id,
            &subject,
            1200,
            false,
        )
        .await
        .unwrap();
        assert_eq!(history.events.len(), 1200);
        for missing in [1..=1, 501..=600, 1025..=1025, 1200..=1200] {
            let broken = messages
                .iter()
                .filter(|message| !missing.contains(&message.sequence))
                .cloned()
                .collect();
            let error = load_prefix(&HistoryReader(broken), &config, &id, &subject, 1200, false)
                .await
                .err()
                .unwrap();
            assert_eq!(error.kind(), EventStoreErrorKind::CorruptHistory);
        }
        assert!(
            load_prefix(
                &HistoryReader(messages.clone()),
                &config,
                &id,
                &subject,
                1024,
                true
            )
            .await
            .is_err()
        );
        for event in messages.iter_mut().filter(|event| event.sequence > 1000) {
            event.payload = b"malformed after snapshot".to_vec().into();
        }
        let prefix = load_prefix(&HistoryReader(messages), &config, &id, &subject, 1000, true)
            .await
            .unwrap();
        assert_eq!(prefix.events.len(), 1000);
    }

    #[tokio::test]
    async fn byte_limited_final_window_carries_lookahead_and_preserves_commit_order() {
        let (config, id) = fixture();
        let subject =
            config.aggregate_subject(id.aggregate_type().as_str(), id.aggregate_id().as_str());
        // Three records in the final window, each larger than the byte budget.
        let mut messages = commit_messages(&config, &id, 0, 1, 0, 1, 2);
        messages.extend(commit_messages(&config, &id, 1, 897, 1, 3, 300 * 1024));
        messages.extend(commit_messages(&config, &id, 4, 4096, 899, 3, 2));
        let reader = HistoryReader(messages);
        let parallel = load_prefix(&reader, &config, &id, &subject, 4098, false)
            .await
            .unwrap();
        let serial = load_prefix_serial(&reader, &config, &id, &subject, 4098, false)
            .await
            .unwrap();
        assert_eq!(parallel.events, serial.events);
        assert_eq!(parallel.events.len(), 7);
    }

    struct CountedHistoryReader {
        history: HistoryReader,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl SubjectReader for CountedHistoryReader {
        async fn next(
            &self,
            subject: &str,
            sequence: u64,
        ) -> Result<Option<StreamMessage>, EventStoreError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.history.next(subject, sequence).await
        }
    }

    #[tokio::test]
    async fn sparse_event_spacing_does_not_multiply_every_lookup_by_lane_count() {
        let (config, id) = fixture();
        let subject =
            config.aggregate_subject(id.aggregate_type().as_str(), id.aggregate_id().as_str());
        let mut messages = Vec::new();
        let mut previous = 0;
        for version in 0_u64..100 {
            let sequence = version
                .checked_mul(1_000_000_000)
                .and_then(|value| value.checked_add(6000))
                .unwrap();
            messages.extend(commit_messages(
                &config, &id, version, sequence, previous, 1, 2,
            ));
            previous = sequence;
        }
        let reader = CountedHistoryReader {
            history: HistoryReader(messages),
            calls: AtomicUsize::new(0),
        };
        let history = load_prefix(&reader, &config, &id, &subject, previous, false)
            .await
            .unwrap();
        assert_eq!(history.events.len(), 100);
        assert!(reader.calls.load(Ordering::SeqCst) <= 116);
    }
}
