# Modular runtime independent acceptance contracts

Test-author baseline: `050f08693170e3c5e2b9879fae9a9c8244a9c21a`.
The parent `CONTRACT-FREEZE.md` (2026-10-15) overrides both architecture reports.
These tests are the pre-implementation handoff, not a claim that the new runtime exists.

## Targets and acceptance IDs

| Cargo package / target | Contract coverage |
| --- | --- |
| `rostfrei / modular_baseline_probes` | **Runnable behavioral RED**: CODEC-4 silently changes genuine historical meaning; AUTH-2/LIFE-4 prerequisite: local response cache ignores authority outage. The latter is an authority-bypass probe, not a claim that baseline has authorization. |
| `rostfrei-nats / modular_physical_probe` | **Runnable behavioral RED**: LIFE-2, native protocol §13.1; increasing rejection bytes grows permanent journal payload from 883 to 96,755 bytes. This is a logical-byte measurement, not a latency benchmark. |
| `rostfrei / modular_service_contract` | API-1 object-safe injected service and coordinated backend; CODEC-6 custom mapped codec, framework refusal, binding-free/generic replay and failed round-trip; RETRY-1/2/3 zero/one/default budgets, real missing-stream conflict, opposite proposals and replay; AUTH-1/2/3/4 authorization order, outage, scope/principal distinction, full route and original bytes; AUDIT-1/2 attempts, phases, terminals, bounded retention, failure/timeout/clear; LIFE-4 local expired projection. |
| `rostfrei-core / modular_outcome_lifecycle` | LIFE-1/2/5 and ATOMIC-4: independent detached hot/cold readers, missing versus unavailable, no forbidden/corrupt fallback, revision binding, actual hot reclamation, cold-copy withdrawal, permanent proposal equality after expiry. |
| `rostfrei-tracer / modular_codec_contract` | CODEC-1/2/3/4/5: historical unit-to-cent semantics, preserved original facts, mixed v1/v2 history, one shared registry for fixture/execution/Preview/private dispatch, opaque bytes, multi-aggregate codecs, collisions and malformed preflight. Preview may use either frozen display option. |
| `rostfrei-http / modular_context_contract` | AUTH-4/5 and LIFE-4: middleware typed extension, spoofed headers ignored, original full-envelope bytes, missing identity 401/challenge, denied 403 before lifecycle disclosure, authorized expired 410/typed code. |
| `rostfrei-nats / modular_native_contract` | AUTH-2/4/5, LIFE-2/3/4, ATOMIC-2/3: schema-4 physical split, expiry declaration/deletion/idempotence, fresh reader, strict integrity/directory audit and actual private-event consumer after expiry, caller-facing response admission, verified worker resolver, capacity-before-CAS, actual TCP final-frame gate after durable preparation, process+broker restart with retained and expired controls. |

The five future-API targets intentionally do not compile on the baseline. There
are no product stubs, unconditional failure assertions, ignored tests, new features
that hide tests, or replacement execution engines. The independent keyed `ColdStore`
owns detached reference/byte entries only; it cannot access an event store or embedded receipt. The
service spy and counting backend delegate to the actual implementation, including
its consuming session.

The three baseline behavioral probes are in separate Cargo binaries so they run
while future interfaces are absent. The physical probe currently opts into the
baseline schema-3 authority through the existing fixture. **When the new protocol
exists, update only this new probe's setup to an isolated explicitly enabled
schema-4 authority** (as `modular_native_contract::split_store` does). Do not change
the legacy fixture globally or remove the size assertion to make it pass.

## Chosen minimal test-facing API

These names are indicative but coherent. An implementer may consistently rename
them to repository conventions and document mechanical test changes. Behavioral
assertions and existing #85/#92 controls remain authoritative. Constructors return
the existing appropriate error type unless a return type is explicitly shown.
Ordinary value types used in assertions implement `Debug`, `Clone`, `Eq` as appropriate.
No API below implies a general cross-store writer coordinator.

### Service and policy (`rostfrei` re-exports)

```rust
#[async_trait]
trait CommandProcessingService: Send + Sync {
    async fn process(&self, caller: &TrustedCommandContext, command: &EncodedCommand)
        -> ServiceReport<ProcessedCommand>;
    async fn lookup(&self, caller: &TrustedCommandContext, command: &EncodedCommand)
        -> ServiceReport<OutcomeLookup<CommandResponse>>;
}
// ServiceReport<T>:
// result(&self) -> &Result<T, CommandProcessorError>
// into_result(self) -> Result<T, CommandProcessorError>
// attempt_id(&self) -> &AttemptId; audit(&self) -> &AuditDelivery
// ProcessedCommand: origin(&self) -> CompletionOrigin;
// outcome(&self) -> &CompletedOutcome<CommandResponse>
enum CompletionOrigin { Committed, Replayed }

// PrincipalId::new(text), SecurityScope::new(text) -> Result; both expose as_str().
// TrustedCommandContext::new(PrincipalId, SecurityScope) -> Self;
// principal() -> &PrincipalId; security_scope() -> &SecurityScope
enum CommandAccess { ReadOutcome, Execute }
enum AuthorizationError { Unauthenticated, Denied, Unavailable, InvalidConfiguration }
#[async_trait]
trait CommandAuthorizer: Send + Sync {
    async fn authorize(&self, caller: &TrustedCommandContext,
        command: &ValidatedCommand, access: CommandAccess) -> Result<(), AuthorizationError>;
}
// ValidatedCommand::original(&self) -> &EncodedCommand (no public constructor).
// AllowAllCommands: explicit policy, not an implicit default.
// CommandProcessorError / CommandBusError:
// authorization_error(&self) -> Option<&AuthorizationError>
```

`CommandProcessor::builder(BoundedContext, Arc<dyn CommandBackend>)` accepts
`event_codecs(Arc<AggregateCodecs>)`,
`rejection_codec(Arc<dyn RejectionCodec<CommandRejection>>)`,
`conflict_retry_policy(Arc<dyn ConflictRetryPolicy>)`,
`authorizer(Arc<dyn CommandAuthorizer>)`, `audit_sink(Arc<dyn CommandAuditSink>)`,
`audit_timeout(Duration)`, then `build() -> Result<CommandProcessor, _>`.
Missing authorization policy is a configuration failure. Existing command
`register` remains fallible. Tests use `InMemoryEventStore::new()` as the trusted
in-memory authority; if implementation requires an explicit scoped-memory
constructor, adjust fixture construction consistently rather than infer scope
from untrusted command data.

```rust
trait ConflictRetryPolicy: Send + Sync {
    fn after_conflict(&self, conflict: ConflictRetry) -> ConflictRetryDecision;
}
enum ConflictRetryDecision { Stop, RetryNow, RetryAfter(Duration) }
// BoundedConflictRetries::new(usize); default = three retries after first evaluation.
// Existing CommandExecutor::with_max_conflict_retries remains a convenience policy selector.
```

`MappedCommandRejectionCodec::with_codec(mapper, reverse_decoder,
Arc<dyn RejectionCodec<CommandRejection>>)` preserves the current generic bridge.
`CommandExecutionMetadata::with_security_scope(SecurityScope)` binds low-level
interoperation to the same identity as the authorized service. SecurityScope may
live in core and be re-exported from the facade. No principal-based operation key.

### Audit (`rostfrei`)

```rust
#[async_trait]
trait CommandAuditSink: Send + Sync {
    async fn record(&self, record: &CommandAuditRecord) -> Result<(), AuditError>;
}
enum AuditDelivery { Disabled, Delivered, Failed { /* reported failed records */ } }
enum AuditPhase { Started, Finished }
enum AuditTerminal { Committed, Replayed, Denied /* plus other frozen terminal states */ }
// AuditError::new(message) -> Self
// CommandAuditRecord accessors:
// attempt_id() -> &AttemptId; phase() -> AuditPhase;
// terminal() -> Option<AuditTerminal>; command_id() -> &MessageId;
// original_payload() -> &[u8]; principal() -> &PrincipalId
// InMemoryCommandAuditSink::new(max_records: usize) -> Self
// async records(&self) -> Vec<CommandAuditRecord>; async clear(&self)
```

The tests permit bounded sink eviction or reported delivery failure at capacity;
they require bounded retained records and an unchanged successful command. Timeout
and failed callback delivery remain separate from the command result. There is no
durable audit queue, background worker, or crash-complete audit promise.

### Coordinated backend, immutable marker and details (`rostfrei-core`)

```rust
#[async_trait]
trait OutcomeReader: Send + Sync {
    async fn lookup_outcome(&self, key: &OutcomeKey)
        -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError>;
}
#[async_trait]
trait CommandBackend: EventHistory + OutcomeReader {
    fn authority(&self) -> &CommandAuthority;
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError>;
}
#[async_trait]
trait CommandCommitSession: EventHistory {
    async fn commit_outcome(self: Box<Self>, proposal: EventTransaction)
        -> Result<CompletedCommit, EventStoreError>;
}
#[async_trait]
trait OutcomeLifecycle: Send + Sync {
    async fn expire_details(&self, key: &OutcomeKey, expected: &OutcomeRevision)
        -> Result<OutcomeRevision, OutcomeLifecycleError>;
}
enum OutcomeLookup<T> { Missing, Completed(CompletedOutcome<T>) }
enum OutcomeDetails<T> {
    Available(T), Expired, Archived { reference: ArchiveReference },
    Unavailable { reason: DetailUnavailableReason },
}
// DetailUnavailableReason includes MissingDetails and BackendUnavailable.
// OutcomeKey::new(CommandAuthority, BoundedContextName, OperationId) -> Self
// CompletedOutcome<T>::marker() -> &OutcomeMarker; details() -> &OutcomeDetails<T>
// OutcomeMarker::detail_reference() -> &OutcomeDetailReference;
// OutcomeMarker::revision() -> &OutcomeRevision
// CompletedCommit::is_exact_replay() -> bool;
// CompletedCommit::outcome() -> &CompletedOutcome<EncodedOutcomeDetails>

#[async_trait]
trait OutcomeDetailReader: Send + Sync {
    async fn get(&self, reference: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError>;
}
enum DetailRead { Found(Vec<u8>), AbsentHere /* plus declared archive states */ }
enum DetailReadError { Unavailable, Forbidden /* plus typed integrity/configuration errors */ }
// InMemoryEventStore (same underlying coordinated state on clone):
// detail_reader(&self) -> Arc<dyn OutcomeDetailReader>
// with_detail_readers(self, hot: Arc<dyn OutcomeDetailReader>,
//     cold: Option<Arc<dyn OutcomeDetailReader>>) -> Self
```

The reference itself binds scope/authority/operation; the reader is trusted
infrastructure and not a public authorization endpoint. Exposing the raw detail
reader allows independent exact-copy adapters, not public marker deletion.
Reader substitution never substitutes marker authority, lifecycle authority, or
the commit writer. Expired lookup must precede both readers. Missing/corruption/
permission errors must not become `Missing`. Core executor full-result recovery
returns a typed technical/lifecycle error when the completed result is unavailable;
it cannot manufacture acceptance or reevaluate. The tests intentionally do not
require a new untyped result serialization plugin.

Permanent `OutcomeMarker` identity/reference remains unchanged by expiry. Its
opaque revision binds that exact winning reference, rather than merely an operation
string. `expire_details`'s return may be a lifecycle revision; replay of the same
valid expiry request must be idempotent. Existing input `EventTransaction` still
carries completion; permanent storage must not retain its full payload copy.

### Shared codecs and harnesses

- `AggregateCodecs::register<A,C>(C) -> Result<(), CodecRegistrationError>`;
  `register_json<A>() -> Result<(), CodecRegistrationError>`;
  `replace<A,C>(C) -> Result<(), CodecRegistrationError>`.
- `MessageSeriesEngine::with_codecs(Arc<AggregateCodecs>) -> Self`.
- `DomainEventDispatcher::with_codecs(Arc<AggregateCodecs>) -> Self`; existing
  `register<A,E,H>` uses that shared registry.
- `TracerBuilder::with_event_codecs(self, Arc<AggregateCodecs>) -> Self`, forwarded
  into every simulator/RuntimeBindings instance. This keeps RuntimeBindings private.
- Existing fixtures remain JSON message-series fixtures. A historical JSON object
  has genuinely different meaning from generated current JSON; current command
  writes are opaque bytes. No new arbitrary-byte fixture wire schema is required.
- Preview may report the existing typed `InvalidEventPayload` display error after
  decoding/executing with the configured codec, or return an explicitly tagged
  base64 rendering. It may not retry through generated JSON.

### Adapters

- `InMemoryMessagingAdapter::with_service(Arc<dyn CommandProcessingService>) -> Self`;
  `async dispatch_as(&self, &TrustedCommandContext, EncodedCommand)
  -> Result<CommandBusReceipt, CommandBusError>` is the per-call convenience path.
- `rostfrei_http::context_command_router(BoundedContext,
  Arc<dyn CommandProcessingService>, HttpApiConfig) -> axum::Router` consumes a
  middleware-installed typed `Extension<TrustedCommandContext>` on the existing
  `/contexts/{context}/commands/{command}/schemas/{version}/envelope` route.
  Missing context is 401 (configured challenge), not inferred from headers.
  Expired full result is 410 with `code = rostfrei.command.completed-details-expired`.
  A differently named hook on the existing router is an acceptable mechanical adjustment.
- `CommandBusErrorKind::CompletedDetailsExpired` is a boundary error, not a domain
  rejection or replacement cached wire response.
- `NatsMessagingAdapter::with_outcome_service(self,
  Arc<dyn CommandProcessingService>, TrustedCommandContext) -> Self` selects an
  explicitly service-bound caller admission profile for *both* cached and newly
  received responses.
- `NatsMessagingAdapter::command_handler_with_resolver(
  Arc<dyn CommandProcessingService>, Arc<dyn TrustedCommandContextResolver>)
  -> NatsCommandHandler`.
- `TrustedCommandContextResolver` is object-safe in `rostfrei-nats`, with
  `async resolve(&self, &MessageDelivery<CommandAddress>)
  -> Result<TrustedCommandContext, AuthorizationError>`.
  The verifier fixture checks one application-owned capability and ignores claimed
  principal/tenant metadata. This is not a proposed production credential protocol.

### Native fresh authority setup

`NatsEventStoreConfig` test-facing additions:
`with_fresh_split_outcome_epoch(self, &str) -> Result<Self, _>`,
`with_explicit_detail_expiry(self, bool) -> Result<Self, _>`,
`outcome_detail_stream_name(&self) -> &str`,
`outcome_lifecycle_stream_name(&self) -> &str`.
The existing provisioning function creates/validates the configured stores.
New writes are schema 4; details schema 1; old authorities keep original routing.
Names may change with the native implementation. These methods must not silently
mutate the epoch on an existing schema-3 stream.

The native expiry fixture enables deletion only on owned details, verifies journal
deny-delete/purge remains set, checks immutable journal bytes and lifecycle message
count, then uses a fresh backend and actual private-event consumer. Command/lifecycle
credential separation needs final implementation ACL validation in addition to this
admin-owned fixture. The native preparation test gates a real final NATS frame;
it does not fake a committed result or discard a Rust return value as a crash test.

## Retained controls and commands

Use Rust **1.98.0**, two jobs, `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`, and
`target/release-contract-1.98.0`. Compiler path:
`/home/richard/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin`.

```sh
cargo test --offline --locked -p rostfrei --test modular_baseline_probes
cargo test --offline --locked -p rostfrei --test modular_service_contract
cargo test --offline --locked -p rostfrei-core --test modular_outcome_lifecycle
cargo test --offline --locked -p rostfrei-tracer --test modular_codec_contract
cargo test --offline --locked -p rostfrei-http --test modular_context_contract
python3 scripts/test_release_contracts.py --nats-binary /ABS/PINNED/nats-server \
  --evidence-dir /ABS/OWNED/EVIDENCE -- cargo test --offline --locked \
  -p rostfrei-nats --test modular_native_contract -- --test-threads=1
```

Native tests require an owned authenticated NATS **2.12.15**; no skip-on-unavailable
behavior is allowed. `modular_native_contract` owns and reaps its additional broker
for restart, retaining file store/logs under the runner evidence directory. Its TCP
proxy owns only its loopback socket and one child task. It never restarts the runner
broker or any pre-existing service.

No pre-existing test file is edited by the TDD commit. Keep all prior tests enabled,
with mechanical API adjustments only when necessary:

- Core `--lib`, `completed_outcome_codec`, `event_store_contract`, and facade
  `command_bus`: tracking/escaped/replaced handles, retry/read guards, generic codecs,
  accepted-no-event durability, original evidence and identity.
- Native `completed_outcome_store_contract`, `completed_outcome_faults`,
  `completed_outcome_restart_contract`: full shared conformance, opposite winners,
  framework InvalidRequest read guards, actual final PubAck loss, restart.
- Native `completed_outcome_ordering`, `completed_outcome_fencing`,
  `completed_outcome_migration`, `completed_outcome_unavailable`,
  `completed_outcome_diagnostics`, `review85_r2_protocol`: session incarnation,
  old-writer ACL, legacy evidence, failed rejection persistence, diagnostic versus
  completion, prepared response identity/CAS/generation and immutable business wire.
- The complete remaining #85/#92 native/HTTP/event integrity controls in
  ARCHITECTURE §11 remain required by the final implementation gate.

## Explicit remaining validation gaps

This bounded suite is not exhaustive release clearance. Future API targets cannot
be fully type-checked until product interfaces exist; compile diagnostics establish
API gaps only. Mechanical fixture adaptations may be needed when the old trusted
constructors or combined EventStore methods are deliberately replaced. In particular,
`baseline_processor`/`bus` are used only to encode commands in future tests and can
move to an explicit trusted-service profile without changing evidence/assertions.

Final independent review must still cover: every hidden in-memory proposal/payload
copy reclaimed (a public reader returning Absent alone cannot prove that); native
expiry crash between declaration and deletion and lost expiry/detail acknowledgments;
typed archive-pending/unconfigured archive states; full native reference tamper matrix
(digest/length is covered in memory, but not all scope/schema/kind substitutions);
new schema-3 single legacy scope mapping and unsupported legacy physical expiry;
separate lifecycle credential ACLs; new-schema detail-free history request counts;
replicated/failover behavior and request/storage accounting (PERF-1). The bounded
suite directly covers prepare-before-CAS and the existing actual final-PubAck-loss
controls; it does not claim the other crash windows were exercised.

Automatic archive/GC, production cold adapters, FGA providers, full #111 registry,
#107 host integration and durable audit workers remain outside this increment.
