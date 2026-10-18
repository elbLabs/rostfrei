# Typed technical command failures (#85)

Status: implemented for current source kinds; deployment requires upgraded readers.
Shared business-rejection outcomes and the stricter deployment/cutover policy are
specified separately in ADR 0044; this ADR's source/recovery contract is preserved.

## Classification

`CommandProcessorError::execution_failure()` and
`CommandBusError::execution_failure()` expose `ExecutionFailure`. Applications must
inspect this value, not diagnostic strings. The HTTP response exposes the same
`execution` value and computed `recovery`. Internal error text is not sent over
NATS/HTTP. Business rejection remains `CommandResponseOutcome::Rejected`.

| Source | Public source/kind | Recovery | Incoming integration trigger | HTTP |
|---|---|---|---|---|
| Store unavailable | store / unavailable | retry unchanged operation/evidence | delayed NAK | 503 |
| Exhausted optimistic conflicts | store / conflict | retry unchanged operation/evidence | delayed NAK | 503 |
| Corrupt authoritative history | store / corrupt_history | blocked: repair, then explicit redrive | durable quarantine | 500 |
| Capacity exhausted | store / capacity_exhausted | blocked: restore capacity or reduce transaction, then redrive | durable quarantine | 500 |
| Invalid store configuration | store / configuration_mismatch | blocked: fix configuration, then redrive | durable quarantine | 500 |
| Unknown event type | codec / unknown_event_type | blocked: deploy compatible codec, then redrive | durable quarantine | 500 |
| Unsupported event schema | codec / unsupported_schema_version | blocked: deploy compatible codec, then redrive | durable quarantine | 500 |
| Malformed event payload | codec / malformed_payload | blocked: investigated repair, then redrive | durable quarantine | 500 |
| Invalid event envelope | codec / invalid_envelope | blocked: investigated repair, then redrive | durable quarantine | 500 |
| Event encoding failed | codec / encoding_failed | blocked: correct deterministic codec, then redrive | durable quarantine | 500 |
| Operation identity conflict | existing framework Conflict rejection | correct request identity | ordinary rejection ACK | 409 |
| Handler request refusal | existing framework InvalidRequest rejection, guarded within the original attempt | correct caller/handler contract | ordinary rejection ACK after outcome durability | 400 |
| Invalid tracking/finalization or malformed persistence request | processor / invalid_configuration | blocked: repair handler/store contract | durable quarantine | 500 |
| Expected domain rejection | application-mapped rejection | new business attempt uses a new operation ID | ordinary rejection ACK | mapped status |
| Invalid transport envelope | existing InvalidMessage | fix producer; never run handler | durable quarantine | transport error |
| Missing binding / invalid payload | existing framework InvalidRequest rejection | fix request | ordinary rejection ACK | 400 |
| Rejection mapping / processor configuration fault | processor / invalid_configuration | blocked: fix application configuration | durable quarantine | 500 |
| Transport timeout/unavailable | existing Timeout/Unavailable | retry delivery | delayed NAK | 504/503 |

Capacity is **not** corruption. Its source remains distinguishable even though
both require intervention. A transient codec backend must not report a
deterministic codec error: current codec kinds all describe deterministic
incompatibility/encoding failures, not network outages. Reporting a technical failure
does not fabricate events, skip history, or write a completed business outcome.
It is **neither proof of completion nor proof of non-commit**: atomic acceptance
can already be durable when acknowledgement, validation or result materialization
fails. Always reconcile the **same operation and canonical evidence**; never use a
fresh ID to bypass uncertain technical failure.

## Response wire compatibility

Accepted/rejected responses retain schema version 1 and their existing response
identities/encoding. Technical diagnostics use schema version 2 and
`outcome: {status: "execution_failed", value: {source: "store"|"codec"|"processor", kind: ...}}`.
Version/outcome combinations are validated; unknown kinds/versions and unknown
schema-2 fields (including any undeclared recovery extension) fail closed. Recovery
is computed from the source kind. Known fields are decoded with lossless typed visitors,
not a normalized JSON Value: duplicate known envelope/outcome/source fields fail closed
in **both** schemas, including identical duplicates and content-before-tag order.
Intentional schema-1 **unknown extension** compatibility remains; application-owned
rejection details are not redefined as a new globally strict JSON protocol.
The **logical** technical response ID is derived from command ID and typed source.
The **broker publication** ID instead binds the exact bytes, subject and a versioned
transition token in `rostfrei-Control-Response-Transition`: stream name/creation
epoch, global last-sequence generation and observed subject predecessor sequence.
Both identities, token bounds, stream epoch and payload binding are validated on read.
New schema-2 reads and writer reconciliation also require **exactly one** outer
`rostfrei-Control-Correlation-Id` equal to the body's correlation ID. Correlation
metadata is evidence, not authentication authority. Schema-1 readers retain their
historical optional correlation-header contract. Response-only raw storage reads
preserve repeated header lines before validation, avoiding the pinned client's
last-value header conversion; no dependency or other message-family parser is changed.
A→B→A and deletion→A have new generations, not stale JetStream duplicate IDs.
Schema-1 business payloads and broker IDs remain unchanged and immutable.
Existing response address, operation, command, and correlation validation still applies.

This is an additive Rust enum variant but a reader compatibility change:
downstream exhaustive `CommandProcessorErrorKind` and `CommandResponseOutcome`
matches must be updated. Old response readers cannot decode version 2; deploy
updated callers/readers **before** workers that emit diagnostics. Old workers
still lose failure classification. Arbitrary mixed old/new writers are not
claimed to satisfy #85; rollback to old readers is unsafe while v2 diagnostics
remain. Acceptance receipt readers and store schema are unchanged.

This tightens the **unpublished** schema-2 transport contract: earlier experimental
v2 diagnostics without transition headers fail closed and require an audited,
quiesced cutover retaining their source/quarantine evidence. Do not delete or
reinterpret them automatically, or claim arbitrary mixed diagnostic writers work.

## Durability, retention, and recovery

For a blocked execution the native command handler first obtains the technical
response's Publish PubAck **and verifies usable retained publication evidence in the
current stream incarnation**. It then returns the existing `Quarantine` disposition.
The native consumer obtains the quarantine evidence Publish PubAck **before**
confirmed source TERM. An unavailable sink selects retry at the handler level,
**not infinite retry**: the existing consumer delivery budget can durably quarantine
`DeliveryAttemptsExhausted` and TERM even without a typed response. Callers may then
time out. An unavailable quarantine sink still cannot authorize TERM without durable
evidence. A retained blocked diagnostic selects quarantine, never successful ACK.
The original envelope bytes, IDs, and delivery evidence remain in that existing
quarantine stream. Repairing history is a separate, audited operation.

Transient diagnostics are also caller-visible, but the source continues delayed
NAK and is reevaluated on redelivery. A retained transient diagnostic is never a
reason to ACK or stop executing. A later completed response supersedes it.
The publisher permits this transition only with a compare-and-set against the
observed subject sequence. Accepted/rejected responses remain immutable; stale
diagnostics cannot overwrite a completed business response.

`prepare_command_response` produces an immutable `PreparedCommandResponse` containing
token, exact bytes/headers, broker ID and subject CAS. Retry that **same value** with
`publish_prepared_command_response` after uncertain publication; native handler
retries do so. A duplicate PubAck is not fresh durability: success requires the latest
retained exact payload and validated publication identity. A stale/superseded prepared
request is refused, not silently rebased. Re-evaluate/reconcile before preparing a
genuine subsequent transition. The convenience publish API is a fresh snapshot/upsert;
it only re-prepares internally after definite CAS conflict, not timeout/uncertainty.
Lost PubAck followed by an exact retained retry reconciles as duplicate, not fresh.

A fresh nonduplicate PubAck is not sufficient by itself either. The publisher reads
the exact acknowledged sequence, validates its retained bytes/publication identity,
then rechecks the current stream creation epoch. Checking that exact sequence allows
unrelated-subject progress and later valid snapshots/terminal responses. Duplicate or
already-observed retries instead reconcile the **latest** exact publication, preserving
stale-retry refusal. Missing/unusable post-Ack evidence returns error, not fresh success
or an automatic new-token retry. Unknown/old-epoch records remain for audit; they are not
automatically deleted or reinterpreted as business outcomes.

**Administrative precondition:** coordinate quiescence/fencing of response publishers,
all in-flight/retained prepared attempts, affected source consumers, and stream
administration before deletion/recreation, destructive restore, or maintenance redrive.
Drain/cancel old publications and discard prepared requests from the old incarnation;
preserve original command/quarantine/unknown-response audit evidence. Application-owned
deployment/permission fencing must prevent concurrent stream recreation and sequence reuse.
The SDK does **not** implement that administrative lock. Pre/post epoch checks only
observe bounded points in time; an administrator can still remove/recreate storage
after the final observation. They do not control all future admin actions or extend
normal retention/deletion guarantees. This is distinct from reconnect or broker restart
with retained storage, which keeps the stream incarnation. Concurrent same-name admin
recreation is unsupported operationally, but the reproduced in-flight race fails closed
instead of claiming a usable diagnostic on its fresh Ack alone.

Technical responses are transport diagnostics, **not** durable completed-outcome
receipts. They retain the configured response-stream horizon (default 30 days;
bounded byte capacity can refuse writes). The command stream's default two-minute
publication deduplication window is independent of this horizon. The local
adapter/processor never caches execution failures.

Under ADR 0044, if another entry point has already durably completed the unchanged
operation, native handling may publish that authoritative accepted/rejected result
without re-evaluation or diagnostic deletion. With no shared completion, the blocked
diagnostic still requires explicit repair/redrive; repair alone does not invoke the
handler again. Publication still requires usable retained evidence and the same
incarnation checks described above.

For blocked NATS recovery:

1. Retain and inspect the quarantine record. Quiesce affected source consumers,
   response publishers and outstanding prepared attempts; coordinate/fence stream
   administration so the incarnation cannot change during inspection/deletion.
2. Repair the underlying fault; preserve unchanged operation/canonical evidence for
   a technical retry. The diagnostic alone cannot tell whether acceptance committed;
   reconcile authoritative acceptance evidence instead of assuming non-commit.
3. Call `NatsCommandResponseReader::remove_blocked_diagnostic_for_redrive` with the
   original `EncodedCommand`. Pure full-command canonical validation precedes every
   lookup/deletion; response identity alone is insufficient. It deletes exact observed
   blocked diagnostic sequences until none remain (or an older transient diagnostic
   is exposed). It refuses accepted/rejected responses;
   concurrent later business responses in the **same incarnation** are not deleted.
   Sequence numbers alone do not fence administrative recreation. Keep quarantine evidence.
4. Wait out publication deduplication or verify a nonduplicate command Publish
   PubAck. Republish the original bytes/message ID and resume consumers. Command
   deduplication and response deduplication are separate caches: a new prepared
   response uses the retained stream generation after deletion, so no response-window
   wait or response-cache reset is necessary. Discard/quiesce old prepared attempts.
5. Reconcile the later durable business response/acceptance receipt as normal.

Do not treat repeated submission within broker deduplication as a fresh execution.
Do not delete business responses as a repair shortcut. Diagnostic expiry by itself
does not redrive a terminated source: explicit redrive is still necessary. This
maintenance API intentionally does not remove source/quarantine/store records or
claim to coordinate running consumers.

## Evidence and limits

Tests cover processor/local/NATS source kinds, technical wire validation, actual
HTTP sockets, transient recovery, and explicit native redrive without deleting a
business outcome. Existing native quarantine machinery is reused. Saturation and
quarantine-sink recovery tests remain acceptance tests, not known-bug probes.
Review regressions and added controls cover A→B→A, diagnostic deletion/recurrence
within response deduplication, actual response-PubAck loss through a TCP proxy,
retained exact-token retries, stale retries, token validation, competing terminal
writers, and full-command maintenance negatives. A wrapper injects result failure
**after a real NATS acceptance commit** and proves same-identity receipt reconciliation
without a second effect. It is not an injection into the store's internal postcommit
read; that particular failure window is supported by source analysis of
`event_store.rs` after `publish_atomic_batch`. A two-attempt sink-outage control
proves durable exhaustion quarantine with no typed response. No new exhaustion policy,
receipt format, history skipping, or shared rejection-outcome authority is introduced.
Additional raw-byte and native controls cover duplicate known fields, lossless repeated
correlation headers, schema-1 unknown-extension/optional-header compatibility, gated
actual HPUB plus same-name empty-stream recreation, and a real PubAck held while its
owned test record is removed. The direct native-handler race control refuses blocked
quarantine when usable publication verification fails; this is a disposition control,
not an injected source TERM-confirmation loss or an unlimited-retry guarantee.
