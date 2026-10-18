# Completed outcomes and trusted envelope ingress (#92)

Status: implemented, initial **append-only / fail-closed cutover** contract.
Mutable legacy/new writer coexistence and mutable rollback are not supported.
See [the operator runbook](../completed-outcome-cutover.md) before deployment.

## Operation identity and durable horizon

An operation is `(authority namespace, bounded context, operation ID)`. The NATS
authority namespace is its isolated account/application/traffic namespace, not a
transport response stream. Never recycle that namespace or retarget an old retry
to a newly bootstrapped epoch. A technical retry preserves namespace, ID and
canonical evidence. A new attempt after a completed rejection uses a **new ID**.

Execution fingerprint v2 covers context, command type, command schema, canonical
payload and command-provenance mode. Correlation, effective event causation and
original envelope causation are also immutable identity dimensions. New outcomes
record these dimensions, fingerprint version and application/traffic scope.
`created_at` is **audit-only**: the first timestamp is retained, but changing it
does not authorize another evaluation. JSON whitespace/key order is not identity.
Different contexts have separate operation namespaces. Trace headers are not
identity or authentication. Generic direct callers must provide a complete stable
fingerprint. Every new completed decision requires `OutcomeEvidence`. Direct Rust
execution supplies a conservative `rust:<type_name>` / schema-1 / caller-fingerprint-v1
descriptor if none is given. Renaming that Rust type changes identity. Applications
with a durable external protocol should supply a stable explicit descriptor; direct
execution of an existing envelope can use `EncodedCommand::validated_execution_metadata`.

Accepted and rejected outcomes have **no expiry and no ID reuse**. They share the
authoritative event store's append-only retention, deny-delete/purge and capacity
policy. Broker dedup windows and command-response TTL do not expire operations.
Capacity refusal requires capacity restoration, not deletion of outcomes. Memory
store lifetime is its shared state lifetime; it does not claim process durability.

Technical failures are not business rejections and are never cached as completed
outcomes. They prove neither commit nor non-commit. Reconcile the same ID/evidence
after an uncertain result; do not use a fresh ID to bypass it. ADR 0043's typed
source/recovery matrix still applies.

## Core codec, store and session contract

`CommandCompletion` v1 is protocol-neutral: evidence plus acceptance or
`EncodedRejection { codec_id, codec_version, bytes }`. Rejection bytes are bounded
to 64 KiB; codec IDs/evidence are bounded and validated. A `CompletedOutcome`
contains that decision and its transaction record/read guards. Only acceptance
exposes an acceptance receipt. No rejection is a fictitious domain event or an
accepted no-event receipt.

Arbitrary rejection types use `RejectionCodec<R>` through
`CommandExecutor::execute_with_rejection_codec`. `JsonRejectionCodec` is an explicit
opt-in bridge, not a transport cache. Encode, validate and round-trip decode finish
**before persistence**. Unsupported codec/version or malformed replay is a typed
blocked codec failure; the handler is not rerun. The compatibility `execute`
method can execute acceptance, but **fails closed** if a rejection needs an
unconfigured codec. It never silently returns an undurable rejection. This is an
intentional behavioral API change for direct generic users.

For arbitrary domain rejection types shared with processor bindings,
`MappedCommandRejectionCodec` explicitly combines the existing forward rejection
mapper with an application-provided reverse decoder for stored codes/details. It
uses the SAME `rostfrei.command-rejection` v1 format as the processor. Use the
validated envelope metadata with its matching typed command. This supports generic
executor → processor and processor → generic executor replay without serde bounds
on the domain rejection and without requiring a command queue. A one-way mapper
cannot magically reconstruct an arbitrary domain value: retain reverse readers for
old codes. An unrelated custom codec is refused as an unsupported codec, not
silently interpreted as another business outcome.

`CommandProcessor` maps the handler rejection into `CommandRejection` *before*
persistence using its existing mapper and the explicit stable
`rostfrei.command-rejection` v1 codec. Replay does not call the mapper again.
Mapping failure remains processor/invalid_configuration, not a domain rejection.
Handler `InvalidRequest` refusals are mapped inside the original guarded attempt.
An invalid tracking/finalization/persistence contract is instead blocked processor
configuration failure: its missing guards cannot be reconstructed by inventing a
zero-read rejection transaction.
Validated unknown-command/invalid-payload responses are durably recorded too;
identity-conflict responses never replace the existing operation winner.

`EventStore::{load_completed_outcome_in_context,commit_outcome}` and consuming
`AppendSession::commit_outcome` are required trait methods. Custom stores/wrappers
must implement actual atomic durability/forwarding; there is no skipping default.
Existing acceptance-only lookup methods fail with `IdentityConflict` for a
rejected operation, never return an accepted receipt. Low-level append retries
still require exact participant/decision content; executor reconciliation compares
operation evidence and returns the recorded winner even if speculative decisions
differ. Existing accepted receipt schemas remain readable in an active fresh
authority; unaudited legacy authorities have the additional cutover block below.

Acceptance commits its domain events, read guards and outcome atomically. Rejection
discards speculative events but guards **every successful aggregate load**, including
an absent stream, at the atomic decision point. Stale guards reconcile a winner or
rerun the entire handler with the existing bounded conflict budget. Later state
changes do not invalidate a completed rejection. Store refusal cannot return/ACK a
completed rejection. Rejection-write failure retains its typed source but publishes
no response projection; native handling retries or durably quarantines the technical
failure under the existing policy, never acknowledges a completed rejection.

Memory uses the same state lock for version checks and winner insertion. Its
optional outcome capacity independently bounds receipt count. NATS uses the existing
operation-derived transaction subject with expected last subject sequence **zero**,
in the **same atomic batch** as aggregate events/guards. No second decision ledger
or additional unused capacity sink is introduced. Rejections have only guard items
and the final decision item. The existing 100-item limit still applies.

NATS event/legacy acceptance schemas 1–4 / 1–2 are unchanged. New completed decisions
use checksummed transaction schema **3**, including both accepted and rejected
decisions. Old acceptance readers reject this version; they cannot decode rejection
as acceptance. New readers verify checksum, scope, decision/version, participant
shape and atomic provenance. Business response schema 1 remains immutable; schema 2
technical diagnostics and generation-bound publication/CAS are preserved.

## Adapters, uncertainty and migration

Direct generic execution, direct `CommandProcessor`, local, NATS and HTTP consult
the same store authority. `NatsEventStore` needs no command queue for direct replay.
The native response cache is a projection: a worker only ACKs a retained business
response after shared authority agrees. Legacy-only/mismatching responses block;
they are never silently deleted or overwritten. Blocked diagnostics retain ADR
0043's explicit repair/redrive behavior.

`NatsEventStoreConfig` defaults to a **legacy/read-only outcome namespace**. Missing
outcomes and unaudited old acceptance receipts block command evaluation. A new,
isolated namespace explicitly uses `with_fresh_completed_outcome_epoch(...)` at
initial provisioning. Stream metadata durably binds its epoch/version. Connect,
sessions and write APIs verify it. Normal policy update cannot activate an existing
legacy stream or change the epoch. Updated readers without the write epoch may
replay new outcomes but cannot write new decisions or low-level events there.

Quiesced legacy maintenance can call `import_legacy_rejected_outcome` with the
original validated full `EncodedCommand`, expected schema-1 rejection and its
messaging topology. The importer reads and compares the actual retained response
on the **same broker**, then atomically claims the original operation subject.
Payload/causation cannot be inferred from a response hash. Missing, contradictory or
invisible/expired evidence stays blocked. Conflicting legacy acceptance is never
overwritten and remains blocked to command replay. Importing one rejection does
not activate unknown identities. This initial release deliberately provides no
mutable in-place legacy activation or general legacy-acceptance audit tool.

Old binaries cannot be made to honor a new ledger. Operators must quiesce them and
enforce broker credentials/ACLs (or an isolated new account **and new application
namespace**) that exclude old/rollback writers, including direct publishers and
stream-management APIs. A new epoch label alone is not a security fence. Readers
upgrade first. Read-only rollback requires a schema-3-capable reader; arbitrary old
binary mutable rollback is forbidden. Exact responsibilities and native ACL fixture
are in the runbook. Authentication of HTTP callers remains application-owned.

## Trusted full-envelope HTTP ingress

The ordinary payload-only endpoint is unchanged. To preserve original retry bytes,
applications may opt in with
`HttpApiConfig::default().with_trusted_command_envelope_ingress()`:

```
POST /contexts/{context}/commands/{command}/schemas/{version}/envelope
Content-Type: application/json
Idempotency-Key: <original operation ID>

<original CommandEnvelope<RoutedCommand> JSON bytes>
```

Disabled by default (404). Authenticate/authorize this route using application
middleware. Metadata/provenance is evidence, not authority. Configured application/
traffic scope and URL determine the address; route, registration, ID, schema,
correlation/causation, fingerprint, content type and size checks precede dispatch.
Raw envelope bytes are preserved. `CommandBus::dispatch_encoded[_observed]` is the
protocol-independent equivalent; invalid evidence fails before publication/handler.

## Verification and bounded scope

A valid retained blocked technical diagnostic does not override a completion
subsequently recorded through another entry point. Native handling may publish that
shared accepted/rejected winner without invoking a handler and without deleting the
older diagnostic. With no shared completion, repair alone still cannot re-evaluate
the blocked delivery: ADR 0043's explicit repair/redrive policy remains in force.

Once a handler has run, both decisions finalize aggregate tracking and consume the
original guarded append session before winner reconciliation. A concurrent accepted
or rejected record cannot hide forgotten/replaced handles or an observed session
incarnation mismatch. This is a bounded fail-closed check, not an administrative
lock against arbitrary later deletion or recreation.

Enabled tests cover original cross-adapter rejection/restart/capacity/read-guard
regressions; shared memory/NATS store/session arbitration; generic codecs and
fail-closed defaults; accepted/rejected evidence matrices; actual response TTL and
dedup expiry; legacy valid/conflicting/missing evidence; real credential ACL old-
writer/rollback refusal; competing direct acceptance/native rejection; actual atomic
outcome PubAck loss; and actual HTTP response loss for acceptance and rejection.
The independent-process/broker-crash fixture retains file storage. No Docker,
read-model or Studio result is implied by these native tests. Final measured
commands/results and remaining validation limits belong to the development report,
not an assertion that tests prove arbitrary deployment policy or operator behavior.
