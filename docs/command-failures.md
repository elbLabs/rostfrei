# Technical command failures and recovery

Use `CommandProcessorError::execution_failure()` or
`CommandBusError::execution_failure()` to inspect typed `ExecutionFailure` values.
HTTP exposes the same `execution` classification and computed `recovery`.
Diagnostic strings are not a protocol; business rejection remains a separate outcome.

## Classification

| Failure | Recovery | Native delivery | HTTP |
| --- | --- | --- | --- |
| Store unavailable or exhausted optimistic conflicts | Retry original operation/evidence | Delayed NAK | 503 |
| Corrupt history | Repair, then explicit redrive | Quarantine | 500 |
| Capacity exhausted | Restore capacity or address transaction size, then redrive | Quarantine | 500 |
| Store configuration mismatch | Fix configuration, then redrive | Quarantine | 500 |
| Unknown event/schema, malformed payload/envelope, encoding failure | Repair codec/data compatibility, then redrive | Quarantine | 500 |
| Invalid processor, rejection mapper, or tracking/finalization configuration | Repair application contract, then redrive | Quarantine | 500 |
| Operation identity conflict | Correct request identity | Rejection ACK | 409 |
| Invalid request or payload | Correct request | Rejection ACK after required outcome durability | 400 |
| Business rejection | New business attempt uses a new operation ID | Rejection ACK | Mapped status |
| Invalid transport envelope | Fix producer | Quarantine; no handler | Transport error |
| Transport timeout/unavailability | Reconcile original operation | Retry | 504/503 |

A technical failure proves neither completion nor non-commit: acceptance may have
committed before acknowledgement or result recovery failed. Preserve the original
operation and canonical evidence. Changing an uncertain operation's ID can repeat
its effects. Completed-but-unreadable results follow the
[outcome recovery contract](command-runtime.md#identity-and-results).

## Responses and delivery

Business responses use schema 1. Technical responses use schema 2 with
`outcome.status = execution_failed` and typed source/kind. Upgrade readers before
workers emit schema 2; old readers cannot decode it. Keep readers compatible while
these records remain. Technical diagnostics expire with response retention
(default 30 days); they are not permanent completed outcomes.

Transient diagnostics cause retry, never successful ACK. A later completed result
can supersede them. Blocked diagnostics require usable retained response evidence,
then durable quarantine evidence before source TERM. Delivery-budget exhaustion
can quarantine without a typed response, so callers may time out. Quarantine sink
failure cannot authorize TERM without durable evidence.

Prepared response publications bind exact bytes, subject, predecessor, and stream
generation. Retry the same `PreparedCommandResponse` after uncertainty through
`publish_prepared_command_response`; do not manufacture a new transition token.
Both fresh and duplicate PubAcks require retained evidence verification. Stale
prepared attempts cannot overwrite a completed business response.

Schema-2 readers validate transition identity and exactly one outer correlation
header matching the body. Unknown versions/kinds and duplicate known JSON fields
fail closed. The [wire implementation](../crates/rostfrei-nats/src/messaging_adapter.rs)
owns the precise encoding and publication checks.

## Blocked NATS redrive

1. Inspect and retain quarantine and original command evidence. Quiesce affected
   consumers, response publishers, and prepared attempts. Fence stream administration
   against recreation during maintenance.
2. Repair the fault and reconcile authoritative completion using the original
   command. A known winner can be returned without reevaluating the handler.
3. If still blocked without a winner, call
   `NatsCommandResponseReader::remove_blocked_diagnostic_for_redrive` with the
   original `EncodedCommand`. It validates the full command and removes exact
   blocked diagnostic sequences; it refuses accepted/rejected responses. Keep
   source, quarantine, and store evidence.
4. Republish the original bytes/message ID after the command deduplication window,
   or verify a nonduplicate Publish PubAck before resuming consumers. Discard old
   prepared responses. Response deduplication is separate and needs no window wait
   after the diagnostic's generation changes.
5. Reconcile the resulting durable outcome normally.

Repair or diagnostic expiry alone does not redrive a terminated delivery. The
maintenance API does not coordinate workers or administrative locks. Stream
recreation requires drained publications and new handles; reconnect with retained
storage keeps the existing incarnation.
