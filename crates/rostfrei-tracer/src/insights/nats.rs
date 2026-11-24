use std::{collections::BTreeMap, time::Duration};

use async_nats::jetstream::{
    Context,
    context::PublishErrorKind,
    message::PublishMessage,
    stream::{Config, DiscardPolicy, RetentionPolicy, StorageType, Stream},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    AccessScope, AuditEntry, AuditPage, Identity, InsightsError, InsightsJournal, InsightsState,
    JournalSnapshot, Policy, identifier, now_ms,
};

const MAX_JOURNAL_MESSAGE: usize = 512 * 1024;

/// Explicit capacity and privacy-retention contract for one inspection authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NatsInsightsConfig {
    pub replicas: usize,
    pub max_bytes: i64,
    pub retention: Duration,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    version: u32,
    state: Option<InsightsState>,
    audit: AuditEntry,
}

/// One explicitly provisioned, append-only authority per access scope. Native
/// expected-stream-sequence publication orders policy/grant changes and disclosures.
pub struct NatsInsightsJournal {
    context: Context,
    name: String,
    scope: AccessScope,
    incarnation: String,
    limits: NatsInsightsConfig,
}

impl NatsInsightsJournal {
    pub async fn provision(
        context: Context,
        name: &str,
        scope: AccessScope,
        config: NatsInsightsConfig,
    ) -> Result<Self, InsightsError> {
        validate_name(name)?;
        scope.validate()?;
        if !matches!(config.replicas, 1 | 3 | 5)
            || config.max_bytes < 1024 * 1024
            || config.retention.is_zero()
        {
            return Err(InsightsError::Invalid);
        }
        let stream = context
            .create_stream(Config {
                name: name.into(),
                subjects: vec![format!("{name}.state")],
                storage: StorageType::File,
                retention: RetentionPolicy::Limits,
                discard: DiscardPolicy::New,
                max_bytes: config.max_bytes,
                max_age: config.retention,
                max_message_size: i32::try_from(MAX_JOURNAL_MESSAGE)
                    .map_err(|_| InsightsError::Invalid)?,
                num_replicas: config.replicas,
                deny_delete: true,
                deny_purge: true,
                ..Default::default()
            })
            .await
            .map_err(|_| InsightsError::Unavailable)?;
        let journal = Self {
            context,
            name: name.into(),
            scope: scope.clone(),
            incarnation: stream.cached_info().created.to_string(),
            limits: config,
        };
        let state = InsightsState {
            scope: scope.clone(),
            policy: Policy::default(),
            policy_revision: 0,
            requests: BTreeMap::new(),
        };
        let audit = AuditEntry {
            id: identifier(),
            timestamp_ms: now_ms()?,
            identity: Identity {
                issuer: "rostfrei:insights".into(),
                subject: "provisioner".into(),
                client_id: "provisioner".into(),
            },
            scope,
            action: "authority.created".into(),
            outcome: "committed".into(),
            purpose: None,
            policy_revision: 0,
            request_id: None,
            selection: None,
            disclosures: vec![],
        };
        if !journal.commit(0, Some(state), audit).await? {
            return Err(InsightsError::Conflict);
        }
        Ok(journal)
    }

    pub async fn connect(
        context: Context,
        name: &str,
        scope: AccessScope,
    ) -> Result<Self, InsightsError> {
        validate_name(name)?;
        scope.validate()?;
        let stream = context
            .get_stream(name)
            .await
            .map_err(|_| InsightsError::Unavailable)?;
        let journal = Self {
            context,
            name: name.into(),
            scope,
            incarnation: stream.cached_info().created.to_string(),
            limits: NatsInsightsConfig {
                replicas: stream.cached_info().config.num_replicas,
                max_bytes: stream.cached_info().config.max_bytes,
                retention: stream.cached_info().config.max_age,
            },
        };
        journal.load().await?;
        Ok(journal)
    }

    async fn stream(&self) -> Result<Stream, InsightsError> {
        let stream = self
            .context
            .get_stream(&self.name)
            .await
            .map_err(|_| InsightsError::Unavailable)?;
        let info = stream.cached_info();
        let config = &info.config;
        let mut subjects = config.subjects.clone();
        subjects.sort();
        if info.created.to_string() != self.incarnation
            || subjects != [format!("{}.state", self.name)]
            || config.storage != StorageType::File
            || config.retention != RetentionPolicy::Limits
            || config.discard != DiscardPolicy::New
            || config.max_age.is_zero()
            || config.max_age != self.limits.retention
            || config.max_bytes < 1024 * 1024
            || config.max_bytes != self.limits.max_bytes
            || !matches!(config.num_replicas, 1 | 3 | 5)
            || config.num_replicas != self.limits.replicas
            || config.max_messages > 0
            || config.max_messages_per_subject > 0
            || !config.deny_delete
            || !config.deny_purge
            || config.no_ack
            || config.sealed
            || config.mirror.is_some()
            || config
                .sources
                .as_ref()
                .is_some_and(|sources| !sources.is_empty())
        {
            return Err(InsightsError::Unavailable);
        }
        Ok(stream)
    }

    fn decode(&self, bytes: &[u8]) -> Result<Entry, InsightsError> {
        let entry: Entry = serde_json::from_slice(bytes).map_err(|_| InsightsError::Unavailable)?;
        if entry.version != 1
            || entry.audit.scope != self.scope
            || entry
                .state
                .as_ref()
                .is_some_and(|state| state.scope != self.scope)
        {
            return Err(InsightsError::Unavailable);
        }
        Ok(entry)
    }
}

fn validate_name(name: &str) -> Result<(), InsightsError> {
    if name.is_empty()
        || name.len() > 128
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        Err(InsightsError::Invalid)
    } else {
        Ok(())
    }
}

#[async_trait]
impl InsightsJournal for NatsInsightsJournal {
    async fn load(&self) -> Result<JournalSnapshot, InsightsError> {
        for _ in 0..8 {
            let stream = self.stream().await?;
            let revision = stream.cached_info().state.last_sequence;
            let message = stream
                .get_last_raw_message_by_subject(&format!("{}.state", self.name))
                .await
                .map_err(|_| InsightsError::Unavailable)?;
            if message.sequence > revision {
                continue;
            }
            let state = self
                .decode(&message.payload)?
                .state
                .ok_or(InsightsError::Unavailable)?;
            state
                .policy
                .validate()
                .map_err(|_| InsightsError::Unavailable)?;
            return Ok(JournalSnapshot { revision, state });
        }
        Err(InsightsError::Conflict)
    }

    async fn commit(
        &self,
        expected: u64,
        state: Option<InsightsState>,
        audit: AuditEntry,
    ) -> Result<bool, InsightsError> {
        // Both a stream incarnation and its exact predecessor message bind this
        // write. The native message-ID expectation closes recreation between the
        // INFO check and publication, even when the new sequence happens to match.
        let stream = self.stream().await?;
        if stream.cached_info().state.last_sequence != expected {
            return Ok(false);
        }
        if audit.scope != self.scope
            || state
                .as_ref()
                .is_some_and(|state| state.scope != self.scope)
        {
            return Err(InsightsError::Invalid);
        }
        // Each atomic audit entry carries the current authorization snapshot.
        // Native age expiry can remove earlier entries without losing active
        // authority. If the entire window expires, load fails instead of resetting.
        let mut state = if let Some(state) = state {
            state
        } else {
            let snapshot = self.load().await?;
            if snapshot.revision != expected {
                return Ok(false);
            }
            snapshot.state
        };
        state
            .requests
            .retain(|_, request| request.expires_at_ms > audit.timestamp_ms);
        let entry = Entry {
            version: 1,
            state: Some(state),
            audit,
        };
        let bytes = serde_json::to_vec(&entry).map_err(|_| InsightsError::Invalid)?;
        if bytes.len() > MAX_JOURNAL_MESSAGE {
            return Err(InsightsError::Capacity);
        }
        let mut message = PublishMessage::build()
            .payload(bytes.into())
            .message_id(format!("{}:{}", self.incarnation, identifier()))
            .expected_stream(&self.name)
            .expected_last_sequence(expected);
        if expected > 0 {
            let predecessor = stream
                .get_raw_message(expected)
                .await
                .map_err(|_| InsightsError::Unavailable)?;
            let previous_id = predecessor
                .headers
                .get(async_nats::header::NATS_MESSAGE_ID)
                .ok_or(InsightsError::Unavailable)?
                .as_str();
            if !previous_id.starts_with(&format!("{}:", self.incarnation)) {
                return Err(InsightsError::Unavailable);
            }
            message = message.expected_last_message_id(previous_id);
        }
        let ack = self
            .context
            .send_publish(format!("{}.state", self.name), message)
            .await
            .map_err(|_| InsightsError::Unavailable)?;
        match ack.await {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == PublishErrorKind::WrongLastSequence => Ok(false),
            Err(error) if error.kind() == PublishErrorKind::WrongLastMessageId => Ok(false),
            Err(_) => Err(InsightsError::Unavailable),
        }
    }

    async fn audit(&self, before: Option<u64>, limit: usize) -> Result<AuditPage, InsightsError> {
        if !(1..=100).contains(&limit) {
            return Err(InsightsError::Invalid);
        }
        let stream = self.stream().await?;
        let last = stream.cached_info().state.last_sequence;
        let first = stream.cached_info().state.first_sequence;
        let mut cursor = before.map_or(last, |before| before.saturating_sub(1).min(last));
        let mut entries = Vec::new();
        while cursor >= first && cursor > 0 && entries.len() < limit {
            let message = stream
                .get_raw_message(cursor)
                .await
                .map_err(|_| InsightsError::Unavailable)?;
            entries.push(self.decode(&message.payload)?.audit);
            cursor = cursor.saturating_sub(1);
        }
        Ok(AuditPage {
            entries,
            next_before: (cursor >= first && cursor > 0).then(|| cursor.saturating_add(1)),
        })
    }
}
