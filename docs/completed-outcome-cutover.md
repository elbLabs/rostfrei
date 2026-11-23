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
   traffic address for a new history. Isolate its broker account or rotate/restrict
   credentials so old writers cannot publish events, command responses or commands
   there, nor create/update/delete/purge streams or obtain the new credential.
3. Use `NatsEventStoreConfig::for_bounded_context(&new_context)?
   .with_fresh_outcomes()?` only when creating that fresh
   namespace. Provision and verify both journal and outcome-state streams; their
   creation incarnations are cross-bound. No operator epoch label is required.
   Default config intentionally blocks unevidenced
   command execution. The shared host's `with_fresh_outcomes` selects
   this split configuration rather than the low-level inline compatibility mode.
4. Verify old credentials against the broker, including raw direct publish and
   JetStream management. Only then start new writers. Deliver new business attempts
   with new operation IDs. Route old retries to the **original authority namespace**;
   do not translate an old envelope/ID into the new namespace or claim the old retry
   was accepted there. HTTP authorization/routing must enforce the same boundary.
5. Do not give rollback binaries the new credential. A rollback may use an upgraded
   split/actor-capable reader with read-only credentials and
   `with_read_only_outcomes()`; it cannot evaluate new IDs or mutate the new history.
   Broker ACLs, not documentation or ledger metadata, exclude an old binary that
   ignores the authority protocol.

Operator responsibilities: credential issuance/revocation, ACL/account isolation,
quiescence, namespace ownership/no reuse, HTTP authentication/authorization and
preserving/auditing legacy evidence. The library does not administer your broker or
coordinate a deployment lock. Administrative credentials can bypass any fence;
never share them with application/rollback workers.

Framework responsibilities: default fail-closed legacy processing; durable authority
verification; rejected-outcome codecs and atomic guards/winners; permanent decision
retention with explicit result-detail lifecycle; refusal of authority activation/change
via normal policy update; and safe evidence-bound import without overwriting
conflicting authority. A missing/corrupt/unavailable detail record never permits
reevaluation of a completed decision.

## Back up and restore a split authority

`cargo rostfrei store backup` and `cargo rostfrei store restore` use JetStream's
native snapshot protocol directly. Use a Rostfrei CLI build containing these
commands and NATS Server **2.14.10** on both ends. The qualified layout is the
split journal and command-results pair (version 2). No external NATS CLI is required.
On 2.12.15 a snapshot can contain a different creation time from the running
stream's, and a seemingly successful restore can lose its identity on the next
restart. The commands refuse that release. If upgrading an existing deployment,
verify the original authority after the broker restart before taking a backup.

1. **Establish one recovery point.** Stop intake, drain in-flight commands and
   broker-fence writers, detail expiry, provisioning and other administration
   of both streams. Two native snapshots do not make a live, cross-stream
   transaction. With the original application, context, traffic scope, epoch
   (if used) and limits, verify `NatsEventStore::connect` and record the store's
   `authority()`, both configured stream names and representative original
   command envelopes/outcomes. Keep this evidence with the deployment config.
2. Back up to a **new** directory. Set `NATS_URL` explicitly and supply the
   maintenance credentials separately: `NATS_USER` and `NATS_PASSWORD`, `NATS_TOKEN`,
   or `NATS_CREDS` (credentials file). Use `tls://` for TLS, `NATS_CA` for a private
   root, and `NATS_CERT` plus `NATS_KEY` for a client certificate. No CLI context
   or implicit default endpoint is used. For example:

   ```sh
   # JOURNAL and RESULTS are the exact names from the verified store config.
   NATS_URL="$SOURCE_URL" cargo rostfrei store backup "$BACKUP_DIR" "$JOURNAL" "$RESULTS"
   ```

   The command checks the cross-bindings and compares both streams' policies,
   creation times, message counts, byte counts and sequence ranges before and
   after backup, then verifies the creation time and bindings inside each native
   archive. Archive the **whole** successful directory as one immutable,
   access-controlled set: the archives contain historical result bodies. Keep
   a trusted copy of `pair.sha256` separately and compare it before restoring.
   No consumer positions, messaging streams, read
   models or other application resources are backed up by this procedure.
3. **Restore only to a fenced destination** with compatible capacity/placement,
   both stream names absent, and no provisioning or workers. Keep the original
   deployment fenced during cutover: these are two copies of the *same*
   authority. Restore the trusted set with the original names:

   ```sh
   NATS_URL="$DESTINATION_URL" cargo rostfrei store restore "$BACKUP_DIR" "$JOURNAL" "$RESULTS"
   ```

   It checks the entire set and both destination names before restoring, then
   compares each restored stream to its source evidence. **On any failure keep
   workers fenced.** Discard incomplete backups. Preserve a partial restore for
   diagnosis; start again on an empty isolated destination with the same intact
   set. Never combine snapshots from different cuts, recreate the missing half,
   change incarnation metadata or provision a new authority as a repair.
4. Before admitting writes, restart the isolated destination on its restored
   storage. Connect with the original config and
   `with_read_only_outcomes()` (do **not** provision). Verify the original
   `authority()`, history and results of exact retries of the saved envelopes.
   Reconcile commands, decisions, external effects and detail withdrawals after
   the backup point: restoring older history alone cannot guarantee safe retries
   or preserve later expiry. Resume only when the cut is authoritative, dependent
   resources are recovered consistently and only the destination can accept writes.

The [disposable recovery test](development.md#nats-authentication-and-workers)
exercises this procedure with a file-backed, single-node source and destination.
Qualify other layouts, broker versions and deployment topologies before relying on them.

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
