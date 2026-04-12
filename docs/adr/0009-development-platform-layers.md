# ADR 0009: Development platform layers around the kernel

## Status

Accepted.

## Decision

Rostfrei will evolve from its event-sourcing and messaging foundation into an
event-sourced development platform. The platform may provide domain metadata,
handler registration, simulation, inspection, Studio, documentation, and AI
interfaces, but these capabilities are layered around the existing explicit
kernel.

`rostfrei-core` remains responsible for deterministic aggregate execution and
EventStore contracts. Messaging contracts remain broker-neutral. Infrastructure
adapters remain replaceable. Schema, registry, macro, simulation, Studio, and AI
crates may depend on the kernel, but the kernel does not depend on them.

Applications may use the kernel without the platform layers. Studio and AI
interfaces are clients of domain metadata and runtime capabilities; neither is
the source of truth for domain behavior.

## Consequences

The project can offer higher-level ergonomics without coupling aggregates to a
UI, AI provider, web protocol, macro system, or broker. New platform crates must
preserve the dependency direction established by the first release. This adds
workspace and compatibility surface, so each layer is introduced only after its
underlying explicit contract has been proven independently.
