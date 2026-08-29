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

The HTTP control plane mounts dispatch separately from simulation and requires
a distinct bearer capability. Commands opt into dispatch individually through
an asynchronous `DispatchAdapter`. A publication-backed adapter completes the
control-plane operation only after broker confirmation and reports `published`;
it does not report `accepted`, `rejected`, or `appended` without evidence from
the command execution side. The idempotency key is mandatory for dispatch and
is included in a mode-specific request fingerprint.

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
