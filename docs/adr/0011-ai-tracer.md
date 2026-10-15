# ADR 0011: AI adapters use Tracer

## Status

Accepted.

## Decision

rostfrei exposes domain descriptors, stream discovery, operation tracing,
continuous observation, real isolated scenario execution, and authorized
dispatch through the protocol-independent Tracer service. Optional **Simulate
handler** exposes core handler computation for explicit rule diagnostics, with
a distinct uncommitted result and no fabricated observed trace (ADR 0010).
AI-facing adapters, including a possible MCP server, are clients of that service
and do not bypass its capability boundaries.

Tracer uses the runtime registry as its declared domain model. AI
tools do not treat source-code scraping as the canonical description of
aggregates and handlers, and they do not receive a privileged path around normal
authorization. Redaction, environment capabilities, validation, and audit are
enforced by the service before results reach any protocol adapter.

The implemented adapter is HTTP JSON plus SSE; there is no implemented MCP server.
Future transports need not move protocol details into the aggregate kernel.
Read-only inspection, isolated stateful Test execution, and production dispatch
are separate capabilities. Test state is resettable without affecting production,
and dispatch requires a separate capability. Legacy Preview is retired. Handler
simulation is neither a prerequisite nor fallback for Test, and injected
dependencies are not automatically sandboxed. These capabilities can be deployed independently.

## Consequences

Tracer clients observe the same domain definitions and execution results.
New interfaces cannot silently invent different dispatch or security semantics.
Production deployments can expose inspection without exposing dispatch, and
organizations can omit Tracer entirely while continuing to use the
rostfrei kernel and adapters.
