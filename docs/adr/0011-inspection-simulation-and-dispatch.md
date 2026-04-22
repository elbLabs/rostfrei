# ADR 0011: Inspection, simulation, and dispatch are separate capabilities

## Status

Accepted.

## Decision

Developer tooling distinguishes three capabilities: inspect, simulate, and
dispatch. Inspect reads history and reconstructs aggregate state without
mutation. Simulate replays history into an isolated in-memory branch and runs
the normal typed command handler without appending authoritative events or
publishing messages. Dispatch executes or publishes a real command and is
available only through an explicit environment capability.

Aggregate inspection is separate from persistence and event codecs. Aggregates
are not required to implement Serde or expose their internal state as a durable
wire contract. An inspection adapter produces a developer-facing document and
supports explicit field omission and redaction before data leaves the runtime.

Simulation returns the base stream version, command outcome, rejection or
predicted events, and an inspected state difference. A simulated branch never
rewrites authoritative history. Tooling must technically separate simulation
from live dispatch. Live dispatch is disabled unless deployment
configuration, authorization, and auditing permit it.

Tracer mounts Dispatch separately from Simulate and requires a distinct bearer
capability. Test and Dispatch use separate instances of the same asynchronous
`CommandTransport`. A publication-backed transport reports broker confirmation
through `CommandTransportObserver` while it continues waiting for a durable
command response. Tracer completes the operation as accepted or rejected only
from that command-execution response; transported results do not claim local
append evidence or invent a base stream version, while simulations explicitly
report `appended: false`. A transport must report publication before returning,
and Tracer verifies that the observation matches the terminal receipt. The
idempotency key is mandatory for transported commands and is included in a
mode-specific request fingerprint.

## Consequences

Developers and AI tools can explore real histories and test commands without
mutating production data. Sensitive aggregate fields are protected at the
server boundary rather than by client convention. Deterministic aggregate handlers
are directly simulatable; commands involving external effects require a future
execution-journal seam and cannot be represented as safely simulated until that
contract exists.

Local, test, and production deployments can use the same NATS dispatch adapter.
They differ in application scope and resource lifecycle: tests use unique
application-scoped JetStream resources and delete their streams after the run,
while production uses stable operator-owned resources.

The durable response narrows but does not eliminate the execution-to-response
gap. A worker reconciles a matching retained response before aggregate execution,
and event-appending acceptance can use exact event-store replay after a crash.
Rejected and accepted-no-event decisions have no transactional operation receipt
or outbox, however, so a crash after the decision and before response persistence
can cause redelivery to evaluate the decision again. No exactly-once terminal
outcome is claimed. Response immutability and reconciliation are effective only
for the configured command-response retention period and capacity; eviction
removes that persisted guard.
