use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::{InsightsError::Invalid, text};

pub const MAX_RECORDS: usize = 256;
pub const MAX_FIELDS: usize = 64;
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;
pub const MAX_GRANT_SECONDS: u64 = 900;
pub const MAX_REQUESTS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessScope {
    pub tenant: String,
    pub application: String,
    pub environment: String,
}

impl AccessScope {
    pub(crate) fn validate(&self) -> Result<(), InsightsError> {
        if [&self.tenant, &self.application, &self.environment]
            .into_iter()
            .all(|part| text(part, 128))
        {
            Ok(())
        } else {
            Err(Invalid)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub issuer: String,
    pub subject: String,
    pub client_id: String,
}

/// Construct only at a trusted authentication boundary. Never deserialize this from
/// a tool argument, HTTP body, actor attribution or forwarded username header.
#[derive(Clone, Debug)]
pub struct Principal {
    pub(crate) identity: Identity,
    pub(crate) scope: AccessScope,
    pub(crate) roles: BTreeSet<String>,
    pub(crate) valid_until_ms: u64,
}

impl Principal {
    pub fn from_trusted_identity(
        identity: Identity,
        scope: AccessScope,
        roles: BTreeSet<String>,
    ) -> Result<Self, InsightsError> {
        scope.validate()?;
        if !text(&identity.issuer, 2048)
            || !text(&identity.subject, 256)
            || !text(&identity.client_id, 256)
            || roles.len() > 128
            || roles.iter().any(|role| !text(role, 128))
        {
            return Err(Invalid);
        }
        Ok(Self {
            identity,
            scope,
            roles,
            valid_until_ms: u64::MAX,
        })
    }

    /// Authentication deadline supplied by the trusted verifier, never the caller.
    #[must_use]
    pub const fn with_valid_until_ms(mut self, deadline: u64) -> Self {
        self.valid_until_ms = deadline;
        self
    }

    pub const fn identity(&self) -> &Identity {
        &self.identity
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessMode {
    RequestLess,
    RequestOnly,
    #[default]
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceKind {
    Event,
    Command,
    Aggregate,
    Operation,
    ReadModel,
    Diagnostic,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceType {
    pub kind: ResourceKind,
    #[serde(rename = "type")]
    pub type_name: String,
    pub schema_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub source: String,
    pub resource: ResourceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate_id: Option<String>,
    #[serde(default)]
    pub ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterOp {
    Eq,
    Lt,
    Gt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Predicate {
    pub path: String,
    pub op: FilterOp,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConditionalPolicy {
    pub when: Predicate,
    #[serde(default)]
    pub record: Option<AccessMode>,
    #[serde(default)]
    pub fields: BTreeMap<String, AccessMode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TypePolicy {
    pub source: String,
    pub resource: ResourceType,
    #[serde(default)]
    pub record: AccessMode,
    #[serde(default)]
    pub fields: BTreeMap<String, AccessMode>,
    /// Conditions only tighten the base policy.
    #[serde(default)]
    pub conditions: Vec<ConditionalPolicy>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    pub types: Vec<TypePolicy>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Query {
    pub selection: Selection,
    /// Canonical JSON Pointers. Each result is keyed by the requested pointer.
    pub fields: Vec<String>,
    #[serde(default)]
    pub filters: Vec<Predicate>,
    pub purpose: String,
    #[serde(default)]
    pub request_missing: bool,
    #[serde(default)]
    pub request_id: Option<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordRef {
    pub id: String,
    /// Stable source version, not a timestamp generated by the inspection service.
    pub version: String,
}

#[derive(Clone, Debug)]
pub struct InsightRecord {
    pub reference: RecordRef,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightRow {
    pub reference: RecordRef,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub records: Vec<InsightRow>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub audit_id: String,
    pub policy_revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestStatus {
    Pending,
    Approved,
    Rejected,
    Revoked,
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessRequest {
    pub id: String,
    pub identity: Identity,
    pub selection: Selection,
    pub fields: Vec<String>,
    pub filter_fields: Vec<String>,
    pub purpose: String,
    pub records: Vec<RecordRef>,
    pub policy_revision: u64,
    pub status: RequestStatus,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub approver: Option<Identity>,
    pub approved_fields: Vec<String>,
    pub approved_records: Vec<RecordRef>,
    /// Internal binding; filter values are never persisted in clear text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessDecision {
    pub approve: bool,
    #[serde(default)]
    pub duration_seconds: u64,
    pub fields: Option<Vec<String>>,
    pub records: Option<Vec<RecordRef>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Disclosure {
    pub reference: RecordRef,
    pub fields: Vec<String>,
    pub predicate_fields: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: String,
    pub timestamp_ms: u64,
    pub identity: Identity,
    pub scope: AccessScope,
    pub action: String,
    pub outcome: String,
    pub purpose: Option<String>,
    pub policy_revision: u64,
    pub request_id: Option<String>,
    pub selection: Option<Selection>,
    pub disclosures: Vec<Disclosure>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditPage {
    pub entries: Vec<AuditEntry>,
    pub next_before: Option<u64>,
}

/// Journal state is storage-only. Its embedded audit event and change are committed
/// together; read projections never establish authorization authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InsightsState {
    pub scope: AccessScope,
    pub policy: Policy,
    pub policy_revision: u64,
    pub requests: BTreeMap<String, AccessRequest>,
}

#[derive(Clone, Debug)]
pub struct JournalSnapshot {
    pub revision: u64,
    pub state: InsightsState,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum InsightsError {
    #[error("invalid insights request")]
    Invalid,
    #[error("authentication required")]
    Unauthenticated,
    #[error("access not permitted")]
    Forbidden,
    #[error("insights authority or source unavailable")]
    Unavailable,
    #[error("policy or access request changed; read current state before retrying")]
    Conflict,
    #[error("query or authority capacity exceeded")]
    Capacity,
    #[error("approved source versions are no longer available")]
    SourceChanged,
}
