# ADR 0012: Studio and AI share one secured control plane

## Status

Accepted.

## Decision

rostfrei will expose domain descriptors, stream inspection, state
reconstruction, command simulation, operation tracing, scenario execution, and
authorized dispatch through a protocol-independent control-plane service.
rostfrei Studio and AI-facing adapters, including a possible MCP server, are
clients of that same service.

The control plane uses the runtime registry as its declared domain model. AI
tools do not treat source-code scraping as the canonical description of
aggregates and handlers, and they do not receive a privileged path around normal
authorization. Redaction, environment capabilities, validation, and audit are
enforced by the service before results reach any UI or AI protocol adapter.

The API may support HTTP, WebSocket, MCP, or other transports without moving
protocol details into the aggregate kernel. Read, simulate, and dispatch
operations carry distinct capabilities and can be deployed independently.

## Consequences

Humans and AI observe the same domain definitions and execution results. New
interfaces cannot silently invent different dispatch or security semantics.
Production deployments can expose inspection without exposing dispatch, and
organizations can omit the control plane entirely while continuing to use the
rostfrei kernel and adapters.
