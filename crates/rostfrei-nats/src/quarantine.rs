use std::time::Duration;

use async_nats::jetstream::{self, message::StreamMessage, stream::LastRawMessageErrorKind};
use async_trait::async_trait;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use rostfrei_messaging_core::{
    AddressKind, ApplicationName, CommandAddress, MAX_QUARANTINE_PAGE_SIZE, MessageAddress,
    QuarantineDiagnostic, QuarantineEntry, QuarantineFilter, QuarantinePage, QuarantineQuery,
    QuarantineReadError, QuarantineReader, QuarantinedMessage, TrafficScope,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use tokio::time::timeout;

use crate::{
    consumer::{MAX_QUARANTINE_RECORD_BYTES, QuarantineRecord},
    hex::encode_lower_hex,
    messaging_config::{MessagingTopology, traffic_subject_prefix},
    stream_policy::is_stream_not_found,
};

const READ_TIMEOUT: Duration = Duration::from_secs(5);
const PAGE_BYTE_BUDGET: usize = 8 * 1024 * 1024;
const INVALID_RECORD_PREFIX_BYTES: usize = 16 * 1024;
const MAX_CURSOR_BYTES: usize = 2048;

/// Reads only the configured quarantine stream, without creating or advancing consumers.
#[derive(Clone)]
pub struct NatsQuarantineReader {
    context: jetstream::Context,
    topology: MessagingTopology,
}

impl NatsQuarantineReader {
    pub const fn new(context: jetstream::Context, topology: MessagingTopology) -> Self {
        Self { context, topology }
    }

    fn prefix(&self) -> String {
        format!(
            "{}.quarantine",
            traffic_subject_prefix(
                self.topology.application().as_str(),
                self.topology.traffic_scope()
            )
        )
    }

    async fn stream(
        &self,
        continuing: bool,
    ) -> Result<jetstream::stream::Stream, QuarantineReadError> {
        let stream = self
            .context
            .get_stream(self.topology.quarantine_stream().as_str())
            .await
            .map_err(|error| {
                if continuing && is_stream_not_found(&error) {
                    QuarantineReadError::StaleGeneration
                } else {
                    QuarantineReadError::Unavailable
                }
            })?;
        let config = &stream.cached_info().config;
        if config.subjects != [format!("{}.>", self.prefix())]
            || config.mirror.is_some()
            || config.sources.is_some()
            || config.subject_transform.is_some()
        {
            return Err(QuarantineReadError::InvalidTopology);
        }
        Ok(stream)
    }

    fn generation(&self, stream: &jetstream::stream::Stream) -> String {
        let mut hash = Sha256::new();
        hash.update(self.prefix().as_bytes());
        hash.update([0]);
        hash.update(self.topology.quarantine_stream().as_str().as_bytes());
        hash.update([0]);
        hash.update(stream.cached_info().created.to_string().as_bytes());
        encode_lower_hex(hash.finalize())
    }

    async fn check_generation(&self, generation: &str) -> Result<(), QuarantineReadError> {
        if self.generation(&self.stream(true).await?) != generation {
            return Err(QuarantineReadError::StaleGeneration);
        }
        Ok(())
    }

    async fn list_inner(
        &self,
        query: QuarantineQuery,
    ) -> Result<QuarantinePage, QuarantineReadError> {
        if query.limit == 0 || query.limit > MAX_QUARANTINE_PAGE_SIZE {
            return Err(QuarantineReadError::InvalidQuery);
        }
        let filter_subject = filter_subject(&self.prefix(), &query.filter)?;
        let stream = self.stream(query.cursor.is_some()).await?;
        let generation = self.generation(&stream);
        let state = &stream.cached_info().state;
        let mut cursor = query.cursor.as_deref().map_or_else(
            || {
                Ok(Cursor {
                    version: 1,
                    generation: generation.clone(),
                    next: state.first_sequence.max(1),
                    last: state.last_sequence,
                    filter: query.filter.clone(),
                })
            },
            Cursor::decode,
        )?;
        if cursor.generation != generation {
            return Err(QuarantineReadError::StaleGeneration);
        }
        if cursor.filter != query.filter || cursor.next == 0 || cursor.last > state.last_sequence {
            return Err(QuarantineReadError::InvalidCursor);
        }
        let mut items = Vec::new();
        let mut bytes = 0_usize;
        let mut finished = cursor.next > cursor.last;
        while !finished && items.len() < query.limit && bytes < PAGE_BYTE_BUDGET {
            let stored = match stream
                .get_first_raw_message_by_subject(&filter_subject, cursor.next)
                .await
            {
                Ok(stored) if stored.sequence <= cursor.last => stored,
                Ok(_) => {
                    finished = true;
                    break;
                }
                Err(error) if error.kind() == LastRawMessageErrorKind::NoMessageFound => {
                    finished = true;
                    break;
                }
                Err(_) => {
                    self.check_generation(&generation).await?;
                    return Err(QuarantineReadError::Unavailable);
                }
            };
            if stored.sequence < cursor.next {
                return Err(QuarantineReadError::Unavailable);
            }
            bytes = bytes.saturating_add(stored.payload.len());
            finished = stored.sequence == cursor.last;
            cursor.next = stored.sequence.saturating_add(1);
            items.push(self.entry(&stored, &generation)?);
        }
        // A reset during reads must never return records from a replacement stream under old IDs.
        self.check_generation(&generation).await?;
        Ok(QuarantinePage {
            items,
            next_cursor: if finished {
                None
            } else {
                Some(cursor.encode()?)
            },
            retained_messages: state.messages,
            snapshot_sequence: cursor.last,
        })
    }

    async fn get_inner(&self, id: &str) -> Result<QuarantineEntry, QuarantineReadError> {
        let (generation, sequence) = parse_id(id)?;
        let stream = self.stream(true).await?;
        if self.generation(&stream) != generation {
            return Err(QuarantineReadError::StaleGeneration);
        }
        let result = stream.get_raw_message(sequence).await;
        self.check_generation(generation).await?;
        let stored = result.map_err(|error| {
            if error.kind() == LastRawMessageErrorKind::NoMessageFound {
                QuarantineReadError::NotFound
            } else {
                QuarantineReadError::Unavailable
            }
        })?;
        if stored.sequence != sequence {
            return Err(QuarantineReadError::Unavailable);
        }
        self.entry(&stored, generation)
    }

    fn entry(
        &self,
        stored: &StreamMessage,
        generation: &str,
    ) -> Result<QuarantineEntry, QuarantineReadError> {
        if !stored
            .subject
            .as_str()
            .starts_with(&format!("{}.", self.prefix()))
        {
            return Err(QuarantineReadError::InvalidTopology);
        }
        let mut entry = QuarantineEntry {
            id: format!("{generation}.{}", stored.sequence),
            stored_at: stored
                .time
                .format(&Rfc3339)
                .map_err(|_| QuarantineReadError::Unavailable)?,
            subject: stored.subject.to_string(),
            message: None,
            diagnostics: Vec::new(),
            invalid_record_base64: None,
            invalid_record_truncated: false,
        };
        if stored.payload.len() > MAX_QUARANTINE_RECORD_BYTES {
            entry.diagnostics.push(QuarantineDiagnostic::RecordTooLarge);
        } else if let Ok(record) = serde_json::from_slice::<QuarantineRecord>(&stored.payload) {
            let message = QuarantinedMessage::from(record);
            if !self.source_matches(&message, stored.subject.as_str()) {
                entry
                    .diagnostics
                    .push(QuarantineDiagnostic::InvalidSourceAddress);
            }
            if STANDARD.decode(&message.payload_base64).is_err() {
                entry
                    .diagnostics
                    .push(QuarantineDiagnostic::InvalidPayloadEncoding);
            }
            if message.payload_truncated {
                entry
                    .diagnostics
                    .push(QuarantineDiagnostic::PayloadTruncated);
            }
            entry.message = Some(message);
            return Ok(entry);
        } else {
            entry.diagnostics.push(QuarantineDiagnostic::InvalidRecord);
        }
        let prefix = stored
            .payload
            .get(..INVALID_RECORD_PREFIX_BYTES)
            .unwrap_or(&stored.payload);
        entry.invalid_record_base64 = Some(STANDARD.encode(prefix));
        entry.invalid_record_truncated = prefix.len() < stored.payload.len();
        Ok(entry)
    }

    fn source_matches(&self, message: &QuarantinedMessage, subject: &str) -> bool {
        let Ok(address) = MessageAddress::parse(message.address.clone()) else {
            return false;
        };
        address.application() == self.topology.application().as_str()
            && address.traffic_scope() == self.topology.traffic_scope()
            && address.kind() != AddressKind::Query
            && self
                .topology
                .stream_for(address.kind())
                .is_some_and(|stream| stream.as_str() == message.source_stream)
            && subject
                == format!(
                    "{}.{}.{}.{}",
                    self.prefix(),
                    address.kind().segment(),
                    address.context(),
                    address.name()
                )
    }
}

#[async_trait]
impl QuarantineReader for NatsQuarantineReader {
    fn application(&self) -> &ApplicationName {
        self.topology.application()
    }

    fn traffic_scope(&self) -> TrafficScope {
        self.topology.traffic_scope()
    }

    async fn list(&self, query: QuarantineQuery) -> Result<QuarantinePage, QuarantineReadError> {
        timeout(READ_TIMEOUT, self.list_inner(query))
            .await
            .map_err(|_| QuarantineReadError::Timeout)?
    }

    async fn get(&self, id: &str) -> Result<QuarantineEntry, QuarantineReadError> {
        timeout(READ_TIMEOUT, self.get_inner(id))
            .await
            .map_err(|_| QuarantineReadError::Timeout)?
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    generation: String,
    next: u64,
    last: u64,
    filter: QuarantineFilter,
}

impl Cursor {
    fn decode(value: &str) -> Result<Self, QuarantineReadError> {
        if value.len() > MAX_CURSOR_BYTES {
            return Err(QuarantineReadError::InvalidCursor);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| QuarantineReadError::InvalidCursor)?;
        let cursor: Self =
            serde_json::from_slice(&bytes).map_err(|_| QuarantineReadError::InvalidCursor)?;
        if cursor.version != 1 || !valid_generation(&cursor.generation) {
            return Err(QuarantineReadError::InvalidCursor);
        }
        Ok(cursor)
    }

    fn encode(&self) -> Result<String, QuarantineReadError> {
        serde_json::to_vec(self)
            .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
            .map_err(|_| QuarantineReadError::Unavailable)
    }
}

fn valid_generation(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_id(id: &str) -> Result<(&str, u64), QuarantineReadError> {
    let (generation, sequence) = id.split_once('.').ok_or(QuarantineReadError::InvalidId)?;
    let parsed = sequence
        .parse::<u64>()
        .map_err(|_| QuarantineReadError::InvalidId)?;
    if !valid_generation(generation) || parsed == 0 || parsed.to_string() != sequence {
        return Err(QuarantineReadError::InvalidId);
    }
    Ok((generation, parsed))
}

fn filter_subject(prefix: &str, filter: &QuarantineFilter) -> Result<String, QuarantineReadError> {
    if filter == &QuarantineFilter::default() {
        // An unfiltered inspection must also show records on malformed/unknown subjects.
        return Ok(format!("{prefix}.>"));
    }
    CommandAddress::new(
        "validation",
        filter.context.as_deref().unwrap_or("context"),
        filter.name.as_deref().unwrap_or("name"),
    )
    .map_err(|_| QuarantineReadError::InvalidQuery)?;
    // No caller-provided wildcard, application prefix, or arbitrary stream name enters a read.
    Ok(format!(
        "{prefix}.{}.{}.{}",
        filter.kind.map_or(
            "*",
            rostfrei_messaging_core::QuarantineMessageKind::subject_segment
        ),
        filter.context.as_deref().unwrap_or("*"),
        filter.name.as_deref().unwrap_or("*")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_reject_subject_injection() {
        for value in ["*", ">", "other.context", "", "x y"] {
            assert_eq!(
                filter_subject(
                    "app.test.quarantine",
                    &QuarantineFilter {
                        context: Some(value.to_owned()),
                        ..Default::default()
                    }
                ),
                Err(QuarantineReadError::InvalidQuery)
            );
        }
    }

    #[test]
    fn identities_require_generation_and_canonical_nonzero_sequence() {
        let generation = "a".repeat(64);
        for suffix in ["0", "01", "+1", "-1", "1.2", "18446744073709551616"] {
            assert!(parse_id(&format!("{generation}.{suffix}")).is_err());
        }
        assert_eq!(
            parse_id(&format!("{generation}.18446744073709551615"))
                .unwrap()
                .1,
            u64::MAX
        );
        assert!(parse_id("bad.1").is_err());
    }
}
