use std::fmt::Write as _;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rostfrei_messaging_core::{
    AddressKind, CallerMetadata, MessageAddress, QuarantineDiagnostic, QuarantineEntry,
    QuarantineFailureKind, QuarantineMessageKind, QuarantinePage, QuarantineQuery,
    QuarantineReadError, TraceContext,
};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::catalog::encode_path_segment;

pub fn validate_query(query: &QuarantineQuery) -> Result<(), QuarantineReadError> {
    if query.limit == 0
        || query.limit > rostfrei_messaging_core::MAX_QUARANTINE_PAGE_SIZE
        || query
            .cursor
            .as_ref()
            .is_some_and(|cursor| cursor.len() > 2048)
    {
        return Err(QuarantineReadError::InvalidQuery);
    }
    rostfrei_messaging_core::CommandAddress::new(
        "validation",
        query.filter.context.as_deref().unwrap_or("context"),
        query.filter.name.as_deref().unwrap_or("name"),
    )
    .map_err(|_| QuarantineReadError::InvalidQuery)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QuarantineScope {
    Test,
    Production,
}

impl QuarantineScope {
    pub const fn list_href(self) -> &'static str {
        match self {
            Self::Test => "/quarantine/test",
            Self::Production => "/quarantine/production",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum QuarantineInspectionError {
    #[error("quarantine inspection is not configured for this scope")]
    NotConfigured,
    #[error("quarantine inspection capacity is exhausted")]
    CapacityExhausted,
    #[error(transparent)]
    Read(#[from] QuarantineReadError),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineCollection {
    pub application: String,
    pub scope: QuarantineScope,
    pub items: Vec<QuarantineSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_href: Option<String>,
    pub retained_messages: String,
    pub snapshot_sequence: String,
    pub order: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineSummary {
    pub id: String,
    pub detail_href: String,
    pub stored_at: String,
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<QuarantineMessageKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<QuarantineFailureKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_consumer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub payload_truncated: bool,
    pub diagnostics: Vec<QuarantineDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineDetail {
    pub application: String,
    pub scope: QuarantineScope,
    #[serde(flatten)]
    pub summary: QuarantineSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<QuarantineSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<CallerMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_context: Option<TraceContext>,
    pub payload: QuarantinePayload,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineSource {
    pub stream: String,
    pub consumer: String,
    pub source_sequence: String,
    pub consumer_sequence: String,
    pub pending_at_quarantine: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuarantinePayloadContent {
    OriginalMessage,
    QuarantineRecord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuarantinePayloadStatus {
    Json,
    BinaryOrInvalidJson,
    InvalidBase64,
    Truncated,
    Unavailable,
    Redacted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantinePayload {
    pub content: QuarantinePayloadContent,
    pub status: QuarantinePayloadStatus,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl QuarantinePayload {
    /// Removes every content representation and its digest together. Size and truncation remain.
    #[must_use]
    pub fn redacted(mut self) -> Self {
        self.status = QuarantinePayloadStatus::Redacted;
        self.base64 = None;
        self.json = None;
        self.sha256 = None;
        self
    }
}

/// Applied by the service, for every protocol. Defaults expose Test evidence and redact production.
/// A policy returning sanitized JSON should start with `payload.redacted()` to also remove raw bytes.
pub trait QuarantinePayloadPolicy: Send + Sync {
    fn payload(&self, scope: QuarantineScope, payload: QuarantinePayload) -> QuarantinePayload {
        match scope {
            QuarantineScope::Test => payload,
            QuarantineScope::Production => payload.redacted(),
        }
    }

    fn metadata(&self, scope: QuarantineScope, metadata: CallerMetadata) -> Option<CallerMetadata> {
        (scope == QuarantineScope::Test).then_some(metadata)
    }

    fn trace_context(&self, scope: QuarantineScope, context: TraceContext) -> Option<TraceContext> {
        (scope == QuarantineScope::Test).then_some(context)
    }

    fn reason(&self, scope: QuarantineScope, reason: String) -> Option<String> {
        (scope == QuarantineScope::Test).then_some(reason)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultQuarantinePayloadPolicy;

impl QuarantinePayloadPolicy for DefaultQuarantinePayloadPolicy {}

pub fn collection(
    application: &str,
    scope: QuarantineScope,
    query: &QuarantineQuery,
    page: &QuarantinePage,
    policy: &dyn QuarantinePayloadPolicy,
) -> QuarantineCollection {
    QuarantineCollection {
        application: application.to_owned(),
        scope,
        items: page
            .items
            .iter()
            .map(|entry| summary(scope, entry, policy))
            .collect(),
        next_href: page
            .next_cursor
            .as_deref()
            .map(|cursor| next_href(scope, query, cursor)),
        retained_messages: page.retained_messages.to_string(),
        snapshot_sequence: page.snapshot_sequence.to_string(),
        order: "oldest-first",
    }
}

fn next_href(scope: QuarantineScope, query: &QuarantineQuery, cursor: &str) -> String {
    let mut href = format!(
        "{}?limit={}&cursor={}",
        scope.list_href(),
        query.limit,
        encode_path_segment(cursor)
    );
    if let Some(kind) = query.filter.kind {
        let kind = match kind {
            QuarantineMessageKind::Command => "command",
            QuarantineMessageKind::CommandResponse => "command-response",
            QuarantineMessageKind::IntegrationEvent => "integration-event",
        };
        let _ = write!(href, "&kind={kind}");
    }
    for (key, value) in [
        ("context", &query.filter.context),
        ("name", &query.filter.name),
    ] {
        if let Some(value) = value {
            let _ = write!(href, "&{key}={}", encode_path_segment(value));
        }
    }
    href
}

fn summary(
    scope: QuarantineScope,
    entry: &QuarantineEntry,
    policy: &dyn QuarantinePayloadPolicy,
) -> QuarantineSummary {
    let message = entry.message.as_ref();
    let address = message.and_then(|message| MessageAddress::parse(message.address.clone()).ok());
    let kind = address.as_ref().and_then(|address| match address.kind() {
        AddressKind::Command => Some(QuarantineMessageKind::Command),
        AddressKind::CommandResponse => Some(QuarantineMessageKind::CommandResponse),
        AddressKind::IntegrationEvent => Some(QuarantineMessageKind::IntegrationEvent),
        AddressKind::Query => None,
    });
    QuarantineSummary {
        id: entry.id.clone(),
        detail_href: format!("{}/{}", scope.list_href(), encode_path_segment(&entry.id)),
        stored_at: entry.stored_at.clone(),
        subject: entry.subject.clone(),
        message_id: message.map(|message| message.message_id.clone()),
        address: message.map(|message| message.address.clone()),
        kind,
        context: address.as_ref().map(|address| address.context().to_owned()),
        name: address.as_ref().map(|address| address.name().to_owned()),
        reason: message.and_then(|message| policy.reason(scope, message.reason.clone())),
        failure_kind: message.and_then(|message| message.failure_kind),
        attempt: message.map(|message| message.attempt),
        source_consumer: message.map(|message| message.source_consumer.clone()),
        correlation_id: message
            .and_then(|message| message.correlation_id.as_ref())
            .map(|id| id.as_str().to_owned()),
        payload_truncated: message.map_or(entry.invalid_record_truncated, |message| {
            message.payload_truncated
        }),
        diagnostics: entry.diagnostics.clone(),
    }
}

pub fn detail(
    application: &str,
    scope: QuarantineScope,
    entry: QuarantineEntry,
    policy: &dyn QuarantinePayloadPolicy,
) -> QuarantineDetail {
    let summary = summary(scope, &entry, policy);
    let (source, metadata, trace_context, payload) = if let Some(message) = entry.message {
        let source = QuarantineSource {
            stream: message.source_stream,
            consumer: message.source_consumer,
            source_sequence: message.source_sequence.to_string(),
            consumer_sequence: message.consumer_sequence.to_string(),
            pending_at_quarantine: message.pending.to_string(),
        };
        let mut payload = payload_view(
            QuarantinePayloadContent::OriginalMessage,
            Some(message.payload_base64),
            message.payload_truncated,
        );
        payload.original_size = message.payload_size.map(|size| size.to_string());
        payload.sha256 = message.payload_sha256;
        (
            Some(source),
            Some(message.metadata),
            message.trace_context,
            payload,
        )
    } else {
        (
            None,
            None,
            None,
            payload_view(
                QuarantinePayloadContent::QuarantineRecord,
                entry.invalid_record_base64,
                entry.invalid_record_truncated,
            ),
        )
    };
    QuarantineDetail {
        application: application.to_owned(),
        scope,
        summary,
        source,
        metadata: metadata.and_then(|metadata| policy.metadata(scope, metadata)),
        trace_context: trace_context.and_then(|context| policy.trace_context(scope, context)),
        payload: policy.payload(scope, payload),
    }
}

fn payload_view(
    content: QuarantinePayloadContent,
    base64: Option<String>,
    truncated: bool,
) -> QuarantinePayload {
    let (status, json) =
        base64
            .as_ref()
            .map_or((QuarantinePayloadStatus::Unavailable, None), |base64| {
                if truncated {
                    return (QuarantinePayloadStatus::Truncated, None);
                }
                let Ok(bytes) = STANDARD.decode(base64) else {
                    return (QuarantinePayloadStatus::InvalidBase64, None);
                };
                serde_json::from_slice(&bytes).map_or(
                    (QuarantinePayloadStatus::BinaryOrInvalidJson, None),
                    |value| (QuarantinePayloadStatus::Json, Some(value)),
                )
            });
    QuarantinePayload {
        content,
        status,
        truncated,
        base64,
        json,
        original_size: None,
        sha256: None,
    }
}
