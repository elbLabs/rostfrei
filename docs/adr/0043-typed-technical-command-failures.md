# Typed technical command failures (#85)

Status: implemented for current source kinds; deployment requires upgraded readers.
This does **not** implement shared business-rejection outcomes (#92).

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
| Invalid unit-of-work/request | existing framework InvalidRequest rejection | correct caller/handler contract | ordinary rejection ACK | 400 |
| Expected domain rejection | application-mapped rejection | new business attempt uses a new operation ID | ordinary rejection ACK | mapped status |
| Invalid transport envelope | existing InvalidMessage | fix producer; never run handler | durable quarantine | transport error |
| Missing binding / invalid payload | existing framework InvalidRequest rejection | fix request | ordinary rejection ACK | 400 |
| Rejection mapping / processor configuration fault | processor / invalid_configuration | blocked: fix application configuration | durable quarantine | 500 |
| Transport timeout/unavailable | existing Timeout/Unavailable | retry delivery | delayed NAK | 504/503 |

Capacity is **not** corruption. Its source remains distinguishable even though
both require intervention. A transient codec backend must not report a
deterministic codec error: current codec kinds all describe deterministic
incompatibility/encoding failures, not network outages. No execution fault adds
events, skips history, or writes an acceptance/rejection receipt.

## Response wire compatibility

Accepted/rejected responses retain schema version 1 and their existing response
identities/encoding. Technical diagnostics use schema version 2 and
`outcome: {status: "execution_failed", value: {source: "store"|"codec"|"processor", kind: ...}}`.
Version/outcome combinations are validated; unknown kinds/versions fail closed.
The technical response message ID is separately derived from the command ID and
typed source kind. It cannot deduplicate a later business response. Existing
response address, operation, command, and correlation validation still applies.

This is an additive Rust enum variant but a reader compatibility change:
downstream exhaustive `CommandProcessorErrorKind` and `CommandResponseOutcome`
matches must be updated. Old response readers cannot decode version 2; deploy
updated callers/readers **before** workers that emit diagnostics. Old workers
still lose failure classification. Arbitrary mixed old/new writers are not
claimed to satisfy #85; rollback to old readers is unsafe while v2 diagnostics
remain. Acceptance receipt readers and store schema are unchanged.

## Durability, retention, and recovery

For a blocked execution the native command handler first obtains the technical
response's Publish PubAck. It then returns the existing `Quarantine` disposition.
The native consumer obtains the quarantine evidence Publish PubAck **before**
confirmed source TERM. Either unavailable sink leaves the source retryable; a
retained diagnostic on redelivery still selects quarantine, never successful ACK.
The original envelope bytes, IDs, and delivery evidence remain in that existing
quarantine stream. Repairing history is a separate, audited operation.

Transient diagnostics are also caller-visible, but the source continues delayed
NAK and is reevaluated on redelivery. A retained transient diagnostic is never a
reason to ACK or stop executing. A later completed response supersedes it.
The publisher permits this transition only with a compare-and-set against the
observed subject sequence. Accepted/rejected responses remain immutable; stale
diagnostics cannot overwrite a completed business response.

Technical responses are transport diagnostics, **not** durable completed-outcome
receipts. They retain the configured response-stream horizon (default 30 days;
bounded byte capacity can refuse writes). The command stream's default two-minute
publication deduplication window is independent of this horizon. The local
adapter/processor never caches execution failures.

For blocked NATS recovery:

1. Retain and inspect the quarantine record. Quiesce the affected command consumers.
2. Repair the underlying fault; preserve unchanged operation/canonical evidence for
   a technical retry. No business attempt was completed.
3. Call `NatsCommandResponseReader::remove_blocked_diagnostic_for_redrive` with the
   original `EncodedCommand`. It validates identity and deletes exact observed
   blocked diagnostic sequences until none remain (or an older transient diagnostic
   is exposed). It refuses accepted/rejected responses;
   concurrent later business responses are not deleted. Keep quarantine evidence.
4. Wait out publication deduplication or verify a nonduplicate command Publish
   PubAck. Republish the original bytes/message ID and resume consumers.
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
