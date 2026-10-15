# ADR 0010: Inspection, simulation, and dispatch are separate capabilities

## Status

Accepted.

## Decision

The core distinguishes inspection, handler simulation, and real execution.
Inspect reads history and reconstructs aggregate state without mutation.
`CommandExecutor::simulate` remains a focused handler-test primitive: it replays
history and runs the typed handler without appending authoritative events or
creating an authoritative acceptance receipt. Injected handler dependencies are
not universally sandboxed; simulation does not establish end-to-end behavior.
Real execution is available only through an explicit environment capability.

Tracer's user-facing Preview workflow is retired. Tracer discovers contracts,
executes real Test commands/suites, observes actual activity and supports separately
authorized production dispatch. It does not expose core simulation or manufacture
command/event evidence from predictions. Retired HTTP simulation routes return
404, never silently publish. The legacy Rust submission entry point returns
`ModeUnavailable("simulate")` without invoking a handler.

Aggregate inspection is separate from persistence and event codecs. Aggregates
are not required to implement Serde or expose their internal state as a durable
wire contract. An inspection adapter produces a developer-facing document and
supports explicit field omission and redaction before data leaves the runtime.

Core simulation returns the decision and simulated stream participants/events.
A simulated branch never rewrites authoritative history. Historical Tracer
simulation records remain explicitly predicted/uncommitted; readers must not
reinterpret their synthetic identities as observed messages. Live dispatch is
disabled unless deployment configuration and authorization permit it.

Tracer mounts Dispatch separately from Test and requires a distinct bearer
capability. Test and Dispatch use separate instances of the same asynchronous
`CommandTransport`. A publication-backed transport reports broker confirmation
through `CommandTransportObserver` while it continues waiting for a durable
command response. Tracer completes the operation as accepted or rejected only
from that command-execution response; transported results do not claim local
append evidence or invent a base stream version. Historical simulation records
retain `appended: false`. A transport must report publication before returning,
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

Local Test and Dispatch can use the same NATS dispatch adapter. As refined by
ADR 0018, they share one canonical application identity but use disjoint traffic
scopes and JetStream resources. Tests delete only their derived test streams,
while normal Dispatch uses stable operator-owned resources.

The durable response narrows but does not eliminate the execution-to-response
gap. A worker reconciles a matching retained response before aggregate execution,
and both event-appending and event-free acceptance use exact event-store replay
after a crash, including after response retention expires (see
[ADR 0039](0039-durable-event-free-acceptance.md)). Rejected decisions have no
transactional operation receipt or outbox, so a crash after rejection and before
response persistence can cause redelivery to evaluate the decision again. No
exactly-once terminal outcome is claimed for rejections. Response immutability and
response reconciliation are effective only for the configured command-response
retention period and capacity; eviction removes that response guard, but not an
accepted operation's authoritative event-store receipt.
