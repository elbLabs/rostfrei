# ADR 0038: Scoped, read-only quarantine inspection

## Status

Accepted.

## Decision

Quarantine inspection reads retained failed deliveries through a `QuarantineReader`
port in `rostfrei-messaging-core`. The NATS adapter binds a reader to one application,
traffic scope, and quarantine stream. It verifies the stream's subject scope and
uses JetStream message-get requests, without creating consumers or acknowledging
messages. This covers commands, command responses, and integration events. Private
domain-event processing retains its blocking behavior from ADR 0012.

Lists traverse ascending storage sequences up to a captured upper bound. Subject
filters select message kind, context, and name. Each request has a five-second
deadline, a maximum of 100 records, and an eight-MiB scan budget (the final fetched
record can exceed the budget). Continuation cursors bind the filter and snapshot
to the stream generation. IDs combine a generation fingerprint and storage
sequence. Recreating a stream invalidates both IDs and cursors; retention gaps
are skipped and expired detail records return not-found. A snapshot bounds new
arrivals but does not pin records against expiry or deletion.

Test reset clears quarantine along with the test scenario. Test evidence does not
survive reset. Inspection never provisions or repairs missing streams. Counts
describe retained records across the stream, not unresolved incidents or filtered
totals.

New records preserve the transport correlation ID and the known failure category:
invalid source message, exhausted delivery attempts, or handler failure. Existing
records remain readable with these fields absent. These categories and the
recorded reason describe available evidence, not a reconstructed root cause.
Malformed records and payloads return diagnostics instead of aborting a listing.
Malformed record bytes are preserved as a bounded prefix for inspection.

## Consequences

Tracer can expose the same port to HTTP and future Studio clients without a NATS
dependency. Captured evidence types deliberately do not implement serialization;
protocol adapters must construct their response and apply their payload policy.
Raw base64, decoded payloads, malformed-record evidence, and caller metadata all
require the same scope-appropriate visibility policy.

NATS inspection credentials need only stream-info and message-get access for the
configured quarantine stream, plus replies on their inbox subjects. Reader
availability does not depend on an application worker or a live Tracer operation.
Operation and causal identities absent from captured evidence remain unknown.
