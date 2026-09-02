# ADR 0011: AI adapters use Tracer

## Status

Accepted.

## Decision

rostfrei will expose domain descriptors, stream inspection, state
reconstruction, command simulation, operation tracing, scenario execution, and
authorized dispatch through the protocol-independent Tracer service.
AI-facing adapters, including a possible MCP server, are clients of that service
and do not bypass its capability boundaries.

Tracer uses the runtime registry as its declared domain model. AI
tools do not treat source-code scraping as the canonical description of
aggregates and handlers, and they do not receive a privileged path around normal
authorization. Redaction, environment capabilities, validation, and audit are
enforced by the service before results reach any protocol adapter.

The API may support HTTP, WebSocket, MCP, or other transports without moving
protocol details into the aggregate kernel. Read-only simulation, isolated
stateful test execution, and production dispatch are explicit modes. Test state
is resettable without affecting production, and dispatch requires a separate
capability. These operations can be deployed independently.

## Consequences

Tracer clients observe the same domain definitions and execution results.
New interfaces cannot silently invent different dispatch or security semantics.
Production deployments can expose inspection without exposing dispatch, and
organizations can omit Tracer entirely while continuing to use the
rostfrei kernel and adapters.
