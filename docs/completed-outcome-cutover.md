# Completed-outcome deployment and cutover

See [ADR 0044](adr/0044-completed-outcomes-and-envelope-ingress.md). Do not enable
new mutable workers on a legacy authority merely because its response cache is
empty. An expired rejection may be invisible forever.

## New isolated authority

1. Upgrade all response/store readers first (transaction schema 3; technical response
   schema 2). Retain immutable history, command envelopes, responses and quarantine.
2. Provision a **new application namespace**, never reuse the legacy application/
   traffic address for a new epoch. Isolate its broker account or rotate/restrict
   credentials so old writers cannot publish events, command responses or commands
   there, nor create/update/delete/purge streams or obtain the new credential.
3. Use `NatsEventStoreConfig::for_bounded_context(&new_context)?
   .with_fresh_completed_outcome_epoch("epoch92")?` only when creating that fresh
   namespace. Epoch metadata is immutable through normal framework policy updates.
   Default config intentionally blocks unevidenced command execution.
4. Verify old credentials against the broker, including raw direct publish and
   JetStream management. Only then start new writers. Deliver new business attempts
   with new operation IDs. Route old retries to the **original authority namespace**;
   do not translate an old envelope/ID into the new namespace or claim the old retry
   was accepted there. HTTP authorization/routing must enforce the same boundary.
5. Do not give rollback binaries the new credential. A rollback may use an upgraded
   schema-3 reader with read-only credentials/config; it cannot evaluate new IDs or
   mutate the new epoch. Broker ACLs, not documentation or ledger metadata, exclude
   an old binary that ignores the ledger.

Operator responsibilities: credential issuance/revocation, ACL/account isolation,
quiescence, namespace ownership/no reuse, HTTP authentication/authorization and
preserving/auditing legacy evidence. The library does not administer your broker or
coordinate a deployment lock. Administrative credentials can bypass any fence;
never share them with application/rollback workers.

Framework responsibilities: default fail-closed legacy processing; durable epoch
verification; rejected-outcome codecs and atomic guards/winners; immutable outcome
retention; refusal of epoch activation/change via normal policy update; and safe
evidence-bound import without overwriting conflicting authority.

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
Raw acceptance-history readers remain available for audit, not mutable rollback.

## ACL mechanism and executable proof

`crates/rostfrei-nats/tests/completed_outcome_fencing.rs` starts its own authenticated
NATS 2.12.15, retains configuration/storage/logs and waits its exact owned PID. A new
writer creates a shared rejection. Old/rollback credentials permit only stream info,
raw reads, ephemeral history-consumer APIs and inbox subscriptions. They cannot
publish aggregate events, operation receipts, commands, responses or stream updates.
The test sends raw requests (so no new ledger-aware code is needed to reject an old
writer), observes broker errors/no successful PubAck and unchanged stream state,
then proves an upgraded read-only reader can replay but cannot evaluate/write.
Do not copy the fixture's disposable passwords into deployments. Restrict the
history-consumer API grants to the intended streams in production.

## Public API migration

Custom `EventStore` and `AppendSession` implementations must implement the new
required outcome methods atomically. Wrappers must forward them while retaining
their fault-injection/read-session behavior. No default skips rejection durability.
For direct generic handlers, replace rejection-capable `execute(...)` calls with
`execute_with_rejection_codec(..., &your_codec)`; codec IDs/versions are durable
contracts and upgrades must retain readers for stored versions. Processor bindings
use their existing rejection mapper and the framework's explicit v1 mapped codec.
Update exhaustive execution/simulation error matches for rejection-mapping and
rejected-persistence errors; technical failures still expose the #85 typed source.

Do not purge completed outcomes when transport TTL/dedup expires. Provision capacity
for decisions/guards as well as events. Back up the authoritative stream with its
metadata and original namespace. Restoring into a different authority namespace is
a migration requiring explicit identity/routing policy, not transparent retry.
