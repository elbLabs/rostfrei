# Completed-outcome deployment and cutover

Use the [native authority contract](command-runtime.md#native-authority)
and [migration guide](migration-0.1.md). Do not enable new mutable workers
on a legacy authority merely because its response cache is empty. An expired legacy
rejection may be invisible forever.

Event storage has separate [subject deployment requirements](messaging/subjects.mdx#fresh-deployments).

## New isolated authority

1. Upgrade all response/store readers first: current inline/split receipts and
   technical response schema 2, plus [actor-bearing formats](actor-attribution.md)
   wherever attribution is enabled. Retain immutable history, command envelopes,
   responses and quarantine.
2. Provision a **new application namespace**, never reuse the legacy application/
   traffic address for a new epoch. Isolate its broker account or rotate/restrict
   credentials so old writers cannot publish events, command responses or commands
   there, nor create/update/delete/purge streams or obtain the new credential.
3. Use `NatsEventStoreConfig::for_bounded_context(&new_context)?
   .with_fresh_split_outcome_epoch("epoch01")?` only when creating that fresh
   namespace. Provision and verify both journal and outcome-state streams; their
   creation incarnations are cross-bound. Epoch metadata is immutable through
   normal framework policy updates. Default config intentionally blocks unevidenced
   command execution. The shared host's `with_fresh_completed_outcome_epoch` selects
   this split configuration rather than the low-level inline compatibility mode.
4. Verify old credentials against the broker, including raw direct publish and
   JetStream management. Only then start new writers. Deliver new business attempts
   with new operation IDs. Route old retries to the **original authority namespace**;
   do not translate an old envelope/ID into the new namespace or claim the old retry
   was accepted there. HTTP authorization/routing must enforce the same boundary.
5. Do not give rollback binaries the new credential. A rollback may use an upgraded
   split/actor-capable reader with read-only credentials and
   `with_read_only_outcomes()`; it cannot evaluate new IDs or mutate the new epoch.
   Broker ACLs, not documentation or ledger metadata, exclude an old binary that
   ignores the authority protocol.

Operator responsibilities: credential issuance/revocation, ACL/account isolation,
quiescence, namespace ownership/no reuse, HTTP authentication/authorization and
preserving/auditing legacy evidence. The library does not administer your broker or
coordinate a deployment lock. Administrative credentials can bypass any fence;
never share them with application/rollback workers.

Framework responsibilities: default fail-closed legacy processing; durable epoch
verification; rejected-outcome codecs and atomic guards/winners; permanent decision
retention with explicit result-detail lifecycle; refusal of epoch activation/change
via normal policy update; and safe evidence-bound import without overwriting
conflicting authority. A missing/corrupt/unavailable detail record never permits
reevaluation of a completed decision.

## Legacy replay (not mutable in-place activation)

Quiesce and broker-fence old writers first. Inventory retained schema-1 rejection
responses **and their original full command envelopes**. For each visible rejection,
call `NatsEventStore::import_legacy_rejected_outcome(command, expected_response,
messaging_topology)` using a default legacy configuration and maintenance credential.
The importer validates complete command identity, reads the actual retained response
from that broker, compares it exactly and atomically writes its decision. It does not
rerun the handler or assert that old read versions still hold after prerequisite
repair. Import is a historical decision, not a new state evaluation.

If evidence is absent/changed, or an acceptance receipt conflicts, stop and preserve
the identity/namespace block. Unknown IDs and unaudited legacy acceptances remain
blocked; import is **not** a switch enabling all legacy IDs. This conservative initial
API has no in-place mutable activation, cache deletion shortcut, bulk acceptance
audit or inference from hashes. New work goes to the fresh isolated namespace above.
Compatible acceptance-history readers serve audit, not mutable rollback; the
current event reader requires the current subject format.

See the [authenticated fencing test](../crates/rostfrei-nats/tests/completed_outcome_fencing.rs)
for an executable ACL example. Restrict reader grants to the intended streams and
ephemeral history-consumer APIs; exclude publication and stream administration.

## Public API migration

Custom executor backends implement `ExecutionStore`, `OutcomeReader` and consuming
`CommandCommitSession` semantics, committing events, guards and permanent decisions
through one coordinator. Low-level `EventStore` and `AppendSession` compatibility
implementations must retain their required outcome methods atomically. Wrappers
must forward the appropriate ports while retaining their fault-injection/read-session
behavior. No default skips rejection durability.
For direct generic handlers, replace rejection-capable `execute(...)` calls with
`execute_with_rejection_codec(..., &your_codec)`; codec IDs/versions are durable
contracts and upgrades must retain readers for stored versions. Processor bindings
use their existing rejection mapper and the framework's explicit v1 mapped codec.
For direct generic execution of the same externally encoded command, use
`encoded.validated_execution_metadata()` and `MappedCommandRejectionCodec::new(
forward_mapper, reverse_decoder)`. That bridge is compatible with the processor's
stored mapped rejection, in both directions. Without explicit metadata the direct
Rust API records a `rust:<type_name>` schema-1 identity; type refactoring changes
that identity, so use stable explicit evidence for long-lived application protocols.
`commit_outcome` refuses missing decision/command evidence; legacy acceptance uses
its separate `append_transaction` API and old receipt representation.
Update exhaustive execution/simulation error matches for rejection-mapping and
rejected-persistence errors; see [typed technical failures](command-failures.md).

Do not purge completed outcomes when transport TTL/dedup expires. Provision capacity
for decisions/guards and result state as well as events. Back up the paired journal
and outcome-state resources with their metadata and original namespace. Explicit
detail expiry withdraws result bodies while retaining the permanent decision; it
does not erase backups or reopen execution. Restoring into a different authority
namespace is a migration requiring explicit identity/routing policy, not transparent
retry. Unilateral stream recreation and stale handles fail closed.
