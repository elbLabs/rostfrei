use std::{collections::BTreeMap, future::Future, sync::Arc, time::Duration};

use async_trait::async_trait;
use base64::Engine as _;

use super::{
    AccessDecision, AccessMode, AccessRequest, AccessScope, AuditEntry, AuditPage, Disclosure,
    InsightRecord, InsightRow, InsightsError, InsightsState, JournalSnapshot, MAX_FIELDS,
    MAX_GRANT_SECONDS, MAX_RECORDS, MAX_REQUESTS, MAX_RESULT_BYTES, Policy, Principal, Query,
    QueryResult, RequestStatus, Selection, TypePolicy, identifier, now_ms, text,
};

#[async_trait]
pub trait InsightsJournal: Send + Sync {
    async fn load(&self) -> Result<JournalSnapshot, InsightsError>;
    /// Atomically append the audit and optional state iff the authority revision
    /// still matches. Await durable acknowledgement; never queue this in memory.
    async fn commit(
        &self,
        expected: u64,
        state: Option<InsightsState>,
        audit: AuditEntry,
    ) -> Result<bool, InsightsError>;
    async fn audit(&self, before: Option<u64>, limit: usize) -> Result<AuditPage, InsightsError>;
}

#[async_trait]
pub trait InsightsSource: Send + Sync {
    /// Return one complete bounded selection, or fail. Sources must isolate the
    /// supplied scope, validate history and never silently truncate.
    async fn read(
        &self,
        scope: &AccessScope,
        selection: &Selection,
    ) -> Result<Vec<InsightRecord>, InsightsError>;
}

/// Server-owned clock. A caller's timestamp never establishes grant validity.
pub trait InsightsClock: Send + Sync {
    fn now_ms(&self) -> Result<u64, InsightsError>;
}

struct SystemClock;

impl InsightsClock for SystemClock {
    fn now_ms(&self) -> Result<u64, InsightsError> {
        now_ms()
    }
}

#[derive(Clone)]
pub struct Insights {
    scope: AccessScope,
    journal: Arc<dyn InsightsJournal>,
    sources: BTreeMap<String, Arc<dyn InsightsSource>>,
    clock: Arc<dyn InsightsClock>,
    binding_key: ring::hmac::Key,
}

impl Insights {
    /// `binding_key` is a persistent, random 32-byte server secret shared by
    /// replicas. Rotating it invalidates outstanding query bindings.
    pub fn new(
        scope: AccessScope,
        journal: Arc<dyn InsightsJournal>,
        binding_key: &[u8; 32],
    ) -> Result<Self, InsightsError> {
        scope.validate()?;
        Ok(Self {
            scope,
            journal,
            sources: BTreeMap::new(),
            clock: Arc::new(SystemClock),
            binding_key: ring::hmac::Key::new(ring::hmac::HMAC_SHA256, binding_key),
        })
    }

    pub fn with_source(
        mut self,
        name: &str,
        source: Arc<dyn InsightsSource>,
    ) -> Result<Self, InsightsError> {
        if !text(name, 128) || self.sources.contains_key(name) || self.sources.len() >= 32 {
            return Err(InsightsError::Invalid);
        }
        self.sources.insert(name.into(), source);
        Ok(self)
    }

    pub const fn scope(&self) -> &AccessScope {
        &self.scope
    }

    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn InsightsClock>) -> Self {
        self.clock = clock;
        self
    }

    fn authorize(&self, principal: &Principal, role: &str) -> Result<(), InsightsError> {
        self.valid_identity(principal)?;
        if principal.scope == self.scope && principal.roles.contains(role) {
            Ok(())
        } else {
            Err(InsightsError::Forbidden)
        }
    }

    fn valid_identity(&self, principal: &Principal) -> Result<(), InsightsError> {
        if principal.valid_until_ms <= self.clock.now_ms()? {
            return Err(InsightsError::Unauthenticated);
        }
        Ok(())
    }

    async fn audited<T>(
        &self,
        principal: &Principal,
        action: &str,
        operation: impl Future<Output = Result<T, InsightsError>>,
    ) -> Result<T, InsightsError> {
        self.record(principal, &format!("{action}.attempt"), "started")
            .await?;
        let result: Result<T, InsightsError> = async {
            self.valid_identity(principal)?;
            let result = operation.await?;
            self.valid_identity(principal)?;
            Ok(result)
        }
        .await;
        if let Err(error) = &result {
            let suffix = if action == "query" {
                "finished"
            } else {
                "denied"
            };
            self.record(principal, &format!("{action}.{suffix}"), &error.to_string())
                .await?;
        }
        result
    }

    async fn snapshot(&self) -> Result<JournalSnapshot, InsightsError> {
        let snapshot = self.journal.load().await?;
        if snapshot.state.scope != self.scope {
            return Err(InsightsError::Unavailable);
        }
        Ok(snapshot)
    }

    fn entry(
        &self,
        principal: &Principal,
        revision: u64,
        action: &str,
    ) -> Result<AuditEntry, InsightsError> {
        Ok(AuditEntry {
            id: identifier(),
            timestamp_ms: self.clock.now_ms()?,
            identity: principal.identity.clone(),
            scope: self.scope.clone(),
            action: action.into(),
            outcome: "committed".into(),
            purpose: None,
            policy_revision: revision,
            request_id: None,
            selection: None,
            disclosures: vec![],
        })
    }

    pub(super) async fn record(
        &self,
        principal: &Principal,
        action: &str,
        outcome: &str,
    ) -> Result<(), InsightsError> {
        for _ in 0..8 {
            let snapshot = self.snapshot().await?;
            let mut entry = self.entry(principal, snapshot.state.policy_revision, action)?;
            entry.outcome = outcome.into();
            if self.journal.commit(snapshot.revision, None, entry).await? {
                return Ok(());
            }
        }
        Err(InsightsError::Conflict)
    }

    pub async fn set_policy(
        &self,
        principal: &Principal,
        expected_revision: u64,
        policy: Policy,
    ) -> Result<u64, InsightsError> {
        self.audited(
            principal,
            "policy.update",
            self.set_policy_inner(principal, expected_revision, policy),
        )
        .await
    }

    async fn set_policy_inner(
        &self,
        principal: &Principal,
        expected_revision: u64,
        policy: Policy,
    ) -> Result<u64, InsightsError> {
        self.authorize(principal, "insights.policy.manage")?;
        policy.validate()?;
        for _ in 0..8 {
            let snapshot = self.snapshot().await?;
            if snapshot.state.policy_revision != expected_revision {
                return Err(InsightsError::Conflict);
            }
            let mut state = snapshot.state;
            state.policy = policy.clone();
            state.policy_revision = state
                .policy_revision
                .checked_add(1)
                .ok_or(InsightsError::Capacity)?;
            // All outstanding grants are tied to the previous policy revision.
            for request in state.requests.values_mut() {
                request.status = RequestStatus::Revoked;
            }
            let revision = state.policy_revision;
            let entry = self.entry(principal, revision, "policy.updated")?;
            if self
                .journal
                .commit(snapshot.revision, Some(state), entry)
                .await?
            {
                return Ok(revision);
            }
        }
        Err(InsightsError::Conflict)
    }

    pub async fn policy(&self, principal: &Principal) -> Result<(u64, Policy), InsightsError> {
        self.audited(principal, "policy.read", self.policy_inner(principal))
            .await
    }

    async fn policy_inner(&self, principal: &Principal) -> Result<(u64, Policy), InsightsError> {
        self.authorize(principal, "insights.policy.manage")?;
        let snapshot = self.snapshot().await?;
        self.record(principal, "policy.read", "admitted").await?;
        Ok((snapshot.state.policy_revision, snapshot.state.policy))
    }

    pub async fn query(
        &self,
        principal: &Principal,
        mut query: Query,
    ) -> Result<QueryResult, InsightsError> {
        self.audited(principal, "query", async {
            self.authorize(principal, "insights.read")?;
            query.normalize()?;
            if query.request_missing {
                self.authorize(principal, "insights.request")?;
            }
            tokio::time::timeout(Duration::from_secs(15), self.query_inner(principal, &query))
                .await
                .map_err(|_| InsightsError::Unavailable)?
        })
        .await
    }

    async fn read_records(
        &self,
        principal: &Principal,
        query: &Query,
        fingerprint: &str,
    ) -> Result<Vec<InsightRecord>, InsightsError> {
        let source = self
            .sources
            .get(&query.selection.source)
            .ok_or(InsightsError::Forbidden)?;
        let initial = self.snapshot().await?;
        let rule = initial
            .state
            .policy
            .for_selection(&query.selection)
            .ok_or(InsightsError::Forbidden)?;
        if rule.record == AccessMode::Deny {
            return Err(InsightsError::Forbidden);
        }
        validate_grant(
            &initial.state,
            principal,
            query,
            fingerprint,
            self.clock.now_ms()?,
        )?;
        let mut records = source.read(&self.scope, &query.selection).await?;
        records.sort_by(|left, right| left.reference.cmp(&right.reference));
        validate_records(&records)?;
        // Public versions must not expose dictionary-testable content hashes.
        // Sources keep their own versioning; callers see a scope/selection-bound MAC.
        for record in &mut records {
            let bytes = serde_json::to_vec(&(
                "rostfrei:insights:record",
                &self.scope,
                &query.selection,
                &record.reference,
            ))
            .map_err(|_| InsightsError::Unavailable)?;
            record.reference.version = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(ring::hmac::sign(&self.binding_key, &bytes).as_ref());
        }
        Ok(records)
    }

    async fn query_inner(
        &self,
        principal: &Principal,
        query: &Query,
    ) -> Result<QueryResult, InsightsError> {
        let fingerprint = query.fingerprint(&self.binding_key, &self.scope)?;
        let records = self.read_records(principal, query, &fingerprint).await?;
        for _ in 0..8 {
            let snapshot = self.snapshot().await?;
            self.valid_identity(principal)?;
            let now = self.clock.now_ms()?;
            let rule = snapshot
                .state
                .policy
                .for_selection(&query.selection)
                .ok_or(InsightsError::Forbidden)?;
            let grant = validate_grant(&snapshot.state, principal, query, &fingerprint, now)?;
            let selected: Vec<_> = records
                .iter()
                .filter(|record| {
                    grant.is_none_or(|grant| grant.approved_records.contains(&record.reference))
                })
                .collect();
            if let Some(grant) = grant
                && selected.len() != grant.approved_records.len()
            {
                return Err(InsightsError::SourceChanged);
            }
            let (rows, disclosures, restricted) = project(rule, query, &selected, grant);
            let mut state = snapshot.state.clone();
            let mut request_id = query.request_id.clone();
            let mut changed = false;
            let requestable = rule.record_mode(None) == AccessMode::RequestOnly
                || query
                    .fields
                    .iter()
                    .chain(query.filters.iter().map(|p| &p.path))
                    .any(|path| rule.field_mode(path, None) == AccessMode::RequestOnly);
            if restricted && requestable && query.request_missing && grant.is_none() {
                let (id, created) =
                    request_access(&mut state, principal, query, &records, &fingerprint, now)?;
                request_id = Some(id);
                changed = created;
            }
            let mut audit = self.entry(principal, state.policy_revision, "query.disclosure")?;
            audit.outcome = "admitted; delivery unconfirmed".into();
            audit.purpose = Some(query.purpose.clone());
            audit.request_id = request_id.clone();
            audit.selection = Some(query.selection.clone());
            audit.disclosures = disclosures;
            let result = QueryResult {
                records: rows,
                status: if restricted { "partial" } else { "complete" }.into(),
                request_id,
                audit_id: audit.id.clone(),
                policy_revision: state.policy_revision,
            };
            if serde_json::to_vec(&result)
                .map_err(|_| InsightsError::Invalid)?
                .len()
                > MAX_RESULT_BYTES
            {
                return Err(InsightsError::Capacity);
            }
            if self
                .journal
                .commit(snapshot.revision, changed.then_some(state), audit)
                .await?
            {
                let released_at = self.clock.now_ms()?;
                if grant.is_some_and(|grant| grant.expires_at_ms <= released_at) {
                    return Err(InsightsError::Forbidden);
                }
                return Ok(result);
            }
        }
        Err(InsightsError::Conflict)
    }

    pub async fn access_request(
        &self,
        principal: &Principal,
        id: &str,
    ) -> Result<AccessRequest, InsightsError> {
        self.audited(
            principal,
            "access.read",
            self.access_request_inner(principal, id),
        )
        .await
    }

    async fn access_request_inner(
        &self,
        principal: &Principal,
        id: &str,
    ) -> Result<AccessRequest, InsightsError> {
        let snapshot = self.snapshot().await?;
        let request = snapshot
            .state
            .requests
            .get(id)
            .ok_or(InsightsError::Forbidden)?;
        if principal.scope != self.scope
            || (principal.identity != request.identity
                && !principal.roles.contains("insights.approve"))
        {
            return Err(InsightsError::Forbidden);
        }
        let result = public_request(
            request,
            principal.roles.contains("insights.approve"),
            self.clock.now_ms()?,
        );
        self.record(principal, "access.read", "admitted").await?;
        Ok(result)
    }

    pub async fn requests(
        &self,
        principal: &Principal,
    ) -> Result<Vec<AccessRequest>, InsightsError> {
        self.audited(principal, "access.list", self.requests_inner(principal))
            .await
    }

    async fn requests_inner(
        &self,
        principal: &Principal,
    ) -> Result<Vec<AccessRequest>, InsightsError> {
        self.authorize(principal, "insights.approve")?;
        let snapshot = self.snapshot().await?;
        let now = self.clock.now_ms()?;
        let requests = snapshot
            .state
            .requests
            .values()
            .map(|request| public_request(request, true, now))
            .collect();
        self.record(principal, "access.list", "admitted").await?;
        Ok(requests)
    }

    pub async fn decide(
        &self,
        principal: &Principal,
        id: &str,
        decision: AccessDecision,
    ) -> Result<(), InsightsError> {
        self.audited(
            principal,
            "access.decision",
            self.decide_inner(principal, id, decision),
        )
        .await
    }

    async fn decide_inner(
        &self,
        principal: &Principal,
        id: &str,
        decision: AccessDecision,
    ) -> Result<(), InsightsError> {
        self.authorize(principal, "insights.approve")?;
        if decision.approve && !(1..=MAX_GRANT_SECONDS).contains(&decision.duration_seconds) {
            return Err(InsightsError::Invalid);
        }
        for _ in 0..8 {
            let snapshot = self.snapshot().await?;
            let mut state = snapshot.state;
            let now = self.clock.now_ms()?;
            let request = state.requests.get_mut(id).ok_or(InsightsError::Forbidden)?;
            if request.identity.issuer == principal.identity.issuer
                && request.identity.subject == principal.identity.subject
            {
                return Err(InsightsError::Forbidden);
            }
            if request.status != RequestStatus::Pending
                || request.expires_at_ms <= now
                || request.policy_revision != state.policy_revision
            {
                return Err(InsightsError::Conflict);
            }
            let mut permitted_fields = request.fields.clone();
            permitted_fields.extend(request.filter_fields.clone());
            let fields = decision
                .fields
                .clone()
                .unwrap_or_else(|| permitted_fields.clone());
            let records = decision
                .records
                .clone()
                .unwrap_or_else(|| request.records.clone());
            if fields.len() > MAX_FIELDS.saturating_add(8)
                || fields.iter().any(|field| !permitted_fields.contains(field))
                || records.len() > MAX_RECORDS
                || records
                    .iter()
                    .any(|record| !request.records.contains(record))
            {
                return Err(InsightsError::Invalid);
            }
            request.status = if decision.approve {
                RequestStatus::Approved
            } else {
                RequestStatus::Rejected
            };
            request.approver = Some(principal.identity.clone());
            request.approved_fields = fields;
            request.approved_records = records;
            if decision.approve {
                request.expires_at_ms =
                    now.saturating_add(decision.duration_seconds.saturating_mul(1000));
            }
            let mut entry = self.entry(
                principal,
                state.policy_revision,
                if decision.approve {
                    "access.approved"
                } else {
                    "access.rejected"
                },
            )?;
            entry.request_id = Some(id.into());
            if self
                .journal
                .commit(snapshot.revision, Some(state), entry)
                .await?
            {
                return Ok(());
            }
        }
        Err(InsightsError::Conflict)
    }

    pub async fn revoke(&self, principal: &Principal, id: &str) -> Result<(), InsightsError> {
        self.audited(principal, "access.revoke", self.revoke_inner(principal, id))
            .await
    }

    async fn revoke_inner(&self, principal: &Principal, id: &str) -> Result<(), InsightsError> {
        self.authorize(principal, "insights.approve")?;
        for _ in 0..8 {
            let snapshot = self.snapshot().await?;
            let mut state = snapshot.state;
            let request = state.requests.get_mut(id).ok_or(InsightsError::Forbidden)?;
            request.status = RequestStatus::Revoked;
            let mut entry = self.entry(principal, state.policy_revision, "access.revoked")?;
            entry.request_id = Some(id.into());
            if self
                .journal
                .commit(snapshot.revision, Some(state), entry)
                .await?
            {
                return Ok(());
            }
        }
        Err(InsightsError::Conflict)
    }

    pub async fn audit(
        &self,
        principal: &Principal,
        before: Option<u64>,
        limit: usize,
    ) -> Result<AuditPage, InsightsError> {
        self.audited(
            principal,
            "audit.read",
            self.audit_inner(principal, before, limit),
        )
        .await
    }

    async fn audit_inner(
        &self,
        principal: &Principal,
        before: Option<u64>,
        limit: usize,
    ) -> Result<AuditPage, InsightsError> {
        self.authorize(principal, "insights.audit.read")?;
        let result = self.journal.audit(before, limit).await?;
        self.record(principal, "audit.read", "admitted").await?;
        Ok(result)
    }
}

fn request_access(
    state: &mut InsightsState,
    principal: &Principal,
    query: &Query,
    records: &[InsightRecord],
    fingerprint: &str,
    now: u64,
) -> Result<(String, bool), InsightsError> {
    if let Some(existing) = state.requests.values().find(|request| {
        request.identity == principal.identity
            && request.fingerprint == fingerprint
            && request.policy_revision == state.policy_revision
            && request.status == RequestStatus::Pending
            && request.expires_at_ms > now
    }) {
        return Ok((existing.id.clone(), false));
    }
    state
        .requests
        .retain(|_, request| request.expires_at_ms > now);
    if state.requests.len() >= MAX_REQUESTS {
        return Err(InsightsError::Capacity);
    }
    let id = identifier();
    let request = AccessRequest {
        id: id.clone(),
        identity: principal.identity.clone(),
        selection: query.selection.clone(),
        fields: query.fields.clone(),
        filter_fields: query.filters.iter().map(|p| p.path.clone()).collect(),
        purpose: query.purpose.clone(),
        records: records.iter().map(|r| r.reference.clone()).collect(),
        policy_revision: state.policy_revision,
        status: RequestStatus::Pending,
        created_at_ms: now,
        expires_at_ms: now.saturating_add(86_400_000),
        approver: None,
        approved_fields: vec![],
        approved_records: vec![],
        fingerprint: fingerprint.into(),
    };
    state.requests.insert(id.clone(), request);
    Ok((id, true))
}

fn public_request(request: &AccessRequest, approver: bool, now: u64) -> AccessRequest {
    let mut result = request.clone();
    result.fingerprint.clear();
    if result.expires_at_ms <= now
        && matches!(
            result.status,
            RequestStatus::Pending | RequestStatus::Approved
        )
    {
        result.status = RequestStatus::Expired;
    }
    // A pending request must not reveal existence/counts of protected records.
    if !approver {
        result.records.clear();
        result.approved_records.clear();
    }
    result
}

fn validate_grant<'a>(
    state: &'a InsightsState,
    principal: &Principal,
    query: &Query,
    fingerprint: &str,
    now: u64,
) -> Result<Option<&'a AccessRequest>, InsightsError> {
    let Some(id) = &query.request_id else {
        return Ok(None);
    };
    let grant = state.requests.get(id).ok_or(InsightsError::Forbidden)?;
    if grant.identity != principal.identity
        || grant.status != RequestStatus::Approved
        || grant.expires_at_ms <= now
        || grant.policy_revision != state.policy_revision
        || grant.fingerprint != fingerprint
    {
        return Err(InsightsError::Forbidden);
    }
    Ok(Some(grant))
}

fn validate_records(records: &[InsightRecord]) -> Result<(), InsightsError> {
    if records.len() > MAX_RECORDS {
        return Err(InsightsError::Capacity);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut bytes = 0usize;
    for record in records {
        if !text(&record.reference.id, 256)
            || !text(&record.reference.version, 256)
            || !seen.insert(&record.reference)
        {
            return Err(InsightsError::Unavailable);
        }
        bytes = bytes.saturating_add(
            serde_json::to_vec(&record.value)
                .map_err(|_| InsightsError::Unavailable)?
                .len(),
        );
        if bytes > MAX_RESULT_BYTES {
            return Err(InsightsError::Capacity);
        }
    }
    Ok(())
}

fn allowed(mode: AccessMode, grant: Option<&AccessRequest>, path: Option<&str>) -> bool {
    match mode {
        AccessMode::RequestLess => true,
        AccessMode::Deny => false,
        AccessMode::RequestOnly => grant.is_some_and(|grant| {
            path.is_none_or(|path| grant.approved_fields.iter().any(|field| field == path))
        }),
    }
}

fn project(
    rule: &TypePolicy,
    query: &Query,
    records: &[&InsightRecord],
    grant: Option<&AccessRequest>,
) -> (Vec<InsightRow>, Vec<Disclosure>, bool) {
    let mut rows = Vec::new();
    let mut disclosures = Vec::new();
    // Determine this from policy, not matched records, to avoid an existence oracle.
    let mut restricted = !allowed(rule.record_mode(None), grant, None)
        || query
            .fields
            .iter()
            .any(|path| !allowed(rule.field_mode(path, None), grant, Some(path)))
        || query
            .filters
            .iter()
            .any(|p| !allowed(rule.field_mode(&p.path, None), grant, Some(&p.path)));
    // Protected predicates pause the entire query; do not evaluate them and then
    // return a permitted field or count that reveals their truth.
    if query
        .filters
        .iter()
        .any(|p| !allowed(rule.field_mode(&p.path, None), grant, Some(&p.path)))
    {
        return (rows, disclosures, true);
    }
    for record in records {
        if !allowed(rule.record_mode(Some(&record.value)), grant, None) {
            continue;
        }
        if !query.filters.iter().all(|p| p.matches(&record.value)) {
            continue;
        }
        let mut fields = BTreeMap::new();
        for path in &query.fields {
            let mode = rule.field_mode(path, Some(&record.value));
            if allowed(mode, grant, Some(path)) {
                if let Some(value) = record.value.pointer(path) {
                    fields.insert(path.clone(), value.clone());
                }
            } else {
                restricted = true;
            }
        }
        disclosures.push(Disclosure {
            reference: record.reference.clone(),
            fields: fields.keys().cloned().collect(),
            predicate_fields: query.filters.iter().map(|p| p.path.clone()).collect(),
        });
        rows.push(InsightRow {
            reference: record.reference.clone(),
            fields,
        });
    }
    (rows, disclosures, restricted)
}
