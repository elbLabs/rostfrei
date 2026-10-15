# Native modular command authority (D2b)

`NatsEventStore` implements `CommandBackend`, `OutcomeReader` and `OutcomeLifecycle`.
`begin_command` creates a consuming session over the existing native history cache
and atomic transaction implementation. `CommandExecutor` remains the retry engine.

## Fresh authority

Select `NatsEventStoreConfig::with_fresh_split_outcome_epoch(epoch)` explicitly,
then provision and connect. This derives disjoint journal/state names and subjects;
it does not move original commands or their histories. Exclude old writers with
broker credentials and route old identities to their original configured authority.
Changing an epoch is a deployment cutover, not an operation retry mechanism.

Two mandatory streams are cross-bound by their creation timestamps in metadata.
The connected authority includes these incarnations, namespace and configured epoch.
Partial provisioning, unilateral recreation, configuration drift and stale handles
fail closed. Split policy updates verify the frozen pair rather than rewriting it.
`with_read_only_outcomes()` supports replay-only handles in the same authority.

The journal retains file-backed events, guards and compact schema-4 manifests.
The state stream uses file/Limits/global DiscardNew, max messages per subject 1,
`discard_new_per_subject=false`, finite byte/message bounds, delete/purge denied,
and no age/TTL/rollup/direct access. It uses the journal's configured capacity,
replicas and message allowance. Admission checks the full state configuration.

## Commit and recovery

Each candidate has one opaque subject derived from its complete immutable reference.
The versioned state envelope contains that reference and either exact immutable
detail bytes (`Active`) or winning revision and previous sequence (`Expired`).
Prepare uses raw expected-subject-sequence **0**, without message IDs, KV operation
headers or rollup. An uncertain ACK reconciles exact bytes and reference. Prepared
storage alone never decides a command; unresolved preparations remain retained.

Events, read guards and the permanent manifest then win through the original atomic
journal CAS. The manifest stores a versioned strict proposal digest, full detail
reference, identity evidence and event-integrity digest, without rejection/command
body or retainable timestamp. Checked restoration validates all required fields,
receipt/key/scope bindings and hashes. Opaque revision v2 binds the complete
reference, proposal digest/version, permanent evidence and event-integrity digest;
`ProposalDigest` itself retains its intentional expected-version exclusion.
Available details are decoded and used to reconstruct and compare the original
proposal/marker before full-result admission.

Every full result reads current leader-served state. One returned Active record
provides lifecycle and body together. Expired refuses hot, cold and transport-cache
results. Missing/corrupt/unavailable state is recovery failure, never permission to
evaluate again. Optional injected detail readers still require current lifecycle
evidence and exact winning-reference validation. No archive exporter is included.
Known durable-result failures use `OutcomeRecovery`; ordinary history and uncertain
writes retain their technical classifications.

## Expiry

Enable `with_explicit_detail_expiry(true)` for private maintenance. Expiry checks
authority, winning revision and current state, then replaces Active with Expired
using its observed sequence. It validates/reconciles the PubAck and bound terminal
state. `Reclaimed` requires a successful prior-sequence `NoMessageFound` check;
failed verification yields `Pending` after proven withdrawal. Unknown transitions
return errors and can be reconciled by repeating the same revision. CAS0 remains
closed after expiry and restart. This is retained-message reclamation, not forensic
erasure of file blocks, backups or responses already issued.

Ordinary history, event reconstruction/consumers, acceptance-receipt validation,
directory listing and deep history audit use permanent evidence independently of
state details. Rejection manifests cannot become acceptance receipts after expiry.

## Service boundaries and legacy

Workers use `command_handler_with_resolver(service, resolver)`; the resolver supplies
a verified `TrustedCommandContext` from the original delivery. The explicit
`ServicePrincipalContext` profile serves trusted infrastructure. Authorization
refusals do not replace shared business responses. Prepared response CAS, diagnostic
uncertainty and bounded quarantine behavior retain the #85 protocol.

Senders use `with_outcome_service(service, caller)`. Each dispatch reauthorizes and
consults the current authoritative winner before exposing cached or newly received
business results. The adapter returns that exact verified winner rather than
treating Active as a permit for arbitrary raw response bytes. Contextless APIs
remain explicit trusted-infrastructure compatibility entry points.

Schema-3 inline reading and evidence-bound legacy import remain supported. Policy
replay needs `with_legacy_security_scope(scope)` or fails identity admission; scope
is configured, never copied from the current caller. Inline expiry is unsupported.
New coordinated command execution requires split storage. Existing low-level
inline interfaces retain their explicit `InlineCompletedOutcome` compatibility type.

The bike-rental epoch option now selects split storage. Test reset recreates its
owned pair and reconnects workers to the new incarnation. Correlation subscriptions
recognize the split aggregate subject layout while excluding outcome subjects.

Independent native review, broad D3 workspace/developer/package/standalone gates
and request/storage accounting follow this development checkpoint.
