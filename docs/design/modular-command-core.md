# Modular command core (D1 checkpoint)

The core implementation is complete at this checkpoint. Processor, facade, fixtures,
Tracer, HTTP and native integration are subsequent D2/D3 work; this is not a
workspace-complete migration.

## Coordinated execution

`CommandExecutor<S>` executes when `S: CommandBackend`. The backend combines
`EventHistory`, `OutcomeReader`, an explicit `CommandAuthority`, and
`begin_command() -> Box<dyn CommandCommitSession>`. The consuming session supplies
attempt-local history and atomically commits the existing `EventTransaction`.
Simulation and rehydration still require only `EventHistory`.

The existing executor remains responsible for tracking/finalization, read guards,
rejection codec round trips, and winner reconciliation. A `ConflictRetryPolicy`
is consulted only after a genuine version conflict with no matching winner.
`BoundedConflictRetries` defaults to three retries after the first evaluation;
zero disables reevaluation, not initial lookup or concurrent-winner reconciliation.
`RetryAfter` awaits the requested delay after the original session is consumed.

`SecurityScope` is supplied by trusted ingress through
`CommandExecutionMetadata::with_security_scope`. It is a permanent identity binding,
not a principal key or an aggregate-storage namespace. The low-level executor and
storage ports are trusted infrastructure, not authorization endpoints.

## Permanent decision versus recoverable detail

`OutcomeKey` is authority + bounded context + operation. A completed marker retains
the immutable detail reference, opaque winning revision, decision kind, bounded
identity evidence, versioned proposal digest, and event-integrity receipt. The
receipt has no `CommandCompletion` body. The first timestamp and encoded rejection
are in `EncodedOutcomeDetails`; accepted event payloads are not duplicated there.

`OutcomeLookup::Missing` alone permits evaluation. A completed marker returns
`OutcomeDetails::{Available, Expired, Archived, Unavailable}`. Missing detail and
backend outage have distinct `DetailUnavailableReason`s. Permission, configuration,
and integrity errors fail closed. The executor returns `OutcomeRecoveryError` for
unrecoverable full results and never substitutes a business rejection or acceptance.
Deliberate expiry is not classified as corrupted history by `execution_failure`.

The in-memory coordinator stores no full proposal and no hidden inline completion.
Its single lock commits events, guards, marker and detail state. Clones retain the
same authority. `with_detail_readers` replaces hot/optional cold readers without
replacing the writer or lifecycle authority. Every full-result invocation checks
current lifecycle before either reader. Absence/outage may fall back to cold;
permission, corruption and configuration errors may not. A missing lifecycle slot
for a completed marker is an error, even when a detached reader has valid bytes.

`OutcomeLifecycle::expire_details(key, expected_revision)` verifies the exact winner
and atomically replaces its active bytes with a payload-free terminal slot. Memory
returns `OutcomeExpiry` with `ReclamationStatus::Reclaimed`; repeated
valid requests are idempotent. The marker/revision, event history and integrity
receipts survive. No API reopens an expired slot. Caller-held or independent cold
copies are outside physical reclamation, but framework resolution will not return
them after expiry. An already-admitted read may finish across concurrent expiry.
There is no automatic TTL, scheduler, archive exporter or orphan collector.

Details format 1 consists of a four-byte big-endian JSON-header length, the JSON
header, and raw rejection bytes. The header binds authority/context/operation/scope,
completion format, evidence and codec identity. The reference binds format, total
length, SHA-256 and decision. Rejection bytes are not JSON numeric arrays.

`ProposalDigest` format 1 uses SHA-256 with eight-byte big-endian length framing:
domain `rostfrei:proposal:v1`; optional context; operation; command fingerprint;
optional correlation, causation and security scope; exact completion JSON v1;
ordered participant stream identities and ordered batch metadata/events; explicit
participant/batch/event/end tags. Options have `some`/`none` tags. Expected versions
are excluded, preserving the original low-level exact-append contract. Timestamp,
rejection codec/bytes, ordered participant and batch content remain significant.
This digest is deliberately distinct from command identity, which reconciles an
accepted/rejected race to its winner. A framing test vector is in `proposal.rs`.

## Compatibility and shared codecs

The legacy full-result wrapper is now explicitly named `InlineCompletedOutcome`.
`EventStore::load_completed_outcome_in_context`, `commit_outcome` and
`CompletedAppendOutcome` retain inline semantics by materializing from the split
reader, not by retaining a second body. They return typed store errors after
withdrawal. New callers use `OutcomeReader` / `CompletedCommit`. Acceptance-only
integrity receipt methods work without details and still reject rejected markers.
`CompletedOutcome<T>` is the new marker + availability result. Its checked `new`
returns `Result<_, OutcomeRecoveryError>` and accepts owned (`'static`) payloads.
Decoded `EncodedOutcomeDetails` retains its complete immutable reference, not only
the completion body. Opposite-decision, same-decision and same-operation foreign
details fail with `IntegrityMismatch` before any business result can be materialized.
`try_map` preserves unavailable states and checks both input and output encoded
bindings. Mapping through an intermediate application DTO cannot manufacture a
verified encoded pair. An allocation-free `Any` downcast checks this concrete type;
application projection types cannot override the check and need no validation trait.
Mapping errors must support `From<OutcomeRecoveryError>`. Generic projections do
not themselves confer storage authority. `to_inline` rechecks encoded binding.

`CompletedOutcome<EncodedOutcomeDetails>::validate_for(key, metadata)` is the shared
consumer check: complete authority/context/operation key, original identity/scope,
receipt integrity and encoded binding. Executor initial lookup, post-conflict
reconciliation and commit results use it; service lookup uses it before projection.
Wrong returned keys are `CorruptHistory`, never Missing or business rejection.

`TransactionReceipt::validate_proposal` checks ordered participant count/identity,
guard bases, complete batch metadata, event cardinality, coordinates and contents.
`OutcomeMarker::prepare` requires this check. `validate_integrity` separately checks
receipt structure and event identities/provenance without reclaimable details;
replay and future native restoration share that boundary. Preparation alone is
not commit authority. These checks do not change `ProposalDigest`'s intentional
expected-version exclusion or winner identity reconciliation.

`AggregateCodecs` is the existing registry evolved into a typed codec plus derived
wire-aggregate validation index. Both views share the same codec `Arc`.
`register` / `register_json` reject duplicate explicit configuration, including a
second instance of the same Rust codec type. `replace` is explicit, and cannot
replace a different model using the same wire identity. Implicit JSON resolution
does not reserve a slot. `resolve` and erased `validate_history` use the same
configuration; the latter applies original decoded facts without rewriting them.
`CommandExecutor::with_codec` is fallible and performs a standalone copy-on-write
override. The preserved core characterization also demonstrates that the low-level
executor currently permits this after `with_codecs`. The subsequent service review
requires shared-mode refusal (S2); that behavior change and its explicitly authorized
characterization adaptation are queued for the service-repair checkpoint. Composed
runtimes must select replacements before publishing the shared registry.

`DomainEventDispatcher::with_codecs` resolves registered handlers through the shared
registry and rejects per-handler overrides. The standalone dispatcher retains its
explicit low-level codec API. The D2a processor, fixture engine, Preview and private
dispatcher use the same selected registry. Installed processor/fixture/dispatcher
configuration rejects local overrides; parity tests verify subsequent consumers
retain the selected historical meaning after those rejected override attempts.

## Remaining integration

D2a implemented the facade, local/HTTP service and shared codec integration. Core
review R1/R2/O1 remediation is implemented, awaiting independent re-review. The
subsequent service review requires adapter result admission, shared-executor
override refusal, retryability, audit classification and stable recovery-origin
corrections; those remain queued for a separate service-repair checkpoint. The
native continuation must migrate remaining native consumers and supply native
marker/reference wire restoration and authority fencing. Native storage must use
the frozen journal + combined CAS/last-value outcome-state protocol, not the obsolete
three-stream deletion recipe. Lifecycle verification uncertainty needs explicit
native transition/reclamation reporting; memory expiry is one atomic transition.
Native schema-3 reading requires the frozen explicit legacy-scope policy and cannot
claim split-payload reclamation. D3 retains broad regressions, request/storage
accounting, standalone-consumer validation and exact-head independent review.
