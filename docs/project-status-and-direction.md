# Zeitstrahl project status and direction

Status as of 2026-08-26.

## Summary

Zeitstrahl is a Rust event-sourcing and messaging framework built around strict
domain boundaries and NATS JetStream. Its working foundation provides typed
aggregate execution, deterministic replay, optimistic concurrency, exact retry,
atomic event commits, transport-neutral messaging, and NATS adapters.

The project is evolving toward an event-sourced development platform in which
the same explicit domain metadata powers runtime dispatch, testing,
visualization, documentation, and AI-assisted development. The aim is not to
hide the existing kernel behind runtime magic. Higher-level tooling will be
generated from inspectable contracts and remain optional.

## Current implementation

The workspace currently contains four crates:

| Crate | Responsibility |
| --- | --- |
| `zeitstrahl-core` | Aggregates, execution, event-store contracts, and the in-memory reference store |
| `zeitstrahl-messaging-core` | Transport-neutral commands, integration events, queries, envelopes, and delivery contracts |
| `zeitstrahl-nats` | NATS messaging and authoritative JetStream event storage |
| `zeitstrahl-testing` | Aggregate scenarios and reusable event-store contracts |

The aggregate model is deliberately small. Aggregates own typed decisions and
deterministic state transitions. The executor owns load, replay, command
handling, event encoding, append, exact retry detection, and bounded conflict
retry. Aggregate state does not depend on Serde, NATS, clocks, IDs, or storage
handles.

The NATS event store writes one JetStream message per domain event. Multi-event
commits use the NATS ADR-50 atomic batch protocol, with one shared batch identity
and the commit header on the final event. This requires NATS Server 2.12.0 or
newer and supports at most 1,000 events in one commit. Retry correctness is a
persisted Zeitstrahl semantic and does not rely on the finite JetStream message
deduplication window.

Messaging supports typed commands, integration events, and queries; bounded
wire envelopes; PubAck-confirmed publication; durable pull consumers; delayed
retry; bounded delivery attempts; quarantine; and Core NATS request/reply.
Infrastructure provisioning remains an explicit operator action rather than a
service-startup side effect.

## Nexus integration

Nexus currently adopts Zeitstrahl as its messaging foundation through a thin
`nexus-messaging` policy facade. an integrating application uses Zeitstrahl publishing,
consumption, retry, quarantine, and topology validation while retaining its
application-owned addresses and deployment defaults.

The integration does not yet run a Nexus aggregate through Zeitstrahl's
`Executor`, `EventStore`, or `NatsEventStore`. No production aggregate is being
converted solely to demonstrate the framework. The existing an integrating application path
remains command to command to a NATS KV entitlement snapshot.

## Verification

The current local implementation has passed workspace tests and strict clippy,
real NATS 2.12 event-store tests, concurrent append tests, the 1,000-event batch
boundary, atomic capacity-failure tests, exact replay contracts, focused Nexus
tests, Nexus architecture tests, and destructive an integrating application NATS tests including
quarantine behavior.

The work is not released. Zeitstrahl has no configured Git remote, recent
changes remain local, and Nexus still uses temporary local path dependencies.
The Nexus release requires an operator-provided Git URL and a pin to one full
Zeitstrahl commit SHA. Database-backed Nexus SQLx tests also require
`DATABASE_URL`.

## Agreed direction

Zeitstrahl will grow in layers around the stable kernel:

1. An explicit domain descriptor model will describe aggregate types, commands,
   events, schema versions, targets, handlers, rejections, and inspection views.
2. A runtime registry will make those descriptors available to dispatch,
   testing, Studio, documentation, and AI integrations.
3. Procedural macros will automatically contribute annotated aggregates and
   handlers to the runtime registry. Application developers will not maintain a
   module or handler registration list.
4. An aggregate inspector will expose redacted developer views without making
   aggregate state a persisted Serde contract.
5. A simulation runtime will replay real history into an isolated branch and
   execute commands without appending or publishing.
6. Zeitstrahl Studio will visualize event timelines, state at any version,
   state differences, operation metadata, command outcomes, and rejections.
7. A protocol-independent control plane will serve both Studio and AI tools with
   the same authorization, redaction, and audit boundaries.

The three operational modes are deliberately distinct:

| Mode | Effect |
| --- | --- |
| Inspect | Read history and reconstruct state without mutation |
| Simulate | Execute against an isolated in-memory branch without publication or append |
| Dispatch | Execute or publish a real command through an explicitly authorized capability |

## Delivery order

1. Complete the standalone Zeitstrahl release and replace Nexus path
   dependencies with a pinned Git revision.
2. Define and independently prove the descriptor and registry contracts.
3. Add procedural macros and automatic linked registration over those contracts.
4. Add erased command execution, aggregate inspection, redaction, and safe
   simulation.
5. Generate command, event, rejection, and inspection schemas.
6. Build the first event timeline and command laboratory in Zeitstrahl Studio.
7. Expose the control plane through MCP and other AI-facing protocols.
8. Add projection management, schema evolution, snapshots, process managers,
   and external-effect journaling only as concrete use cases require them.

## Architecture decision map

| ADR | Decision |
| --- | --- |
| [0001](adr/0001-ubiquitous-language-and-scope.md) | Ubiquitous language, initial scope, and ownership |
| [0002](adr/0002-aggregate-codec-executor-store.md) | Aggregate, codec, executor, and EventStore boundaries |
| [0003](adr/0003-stream-version-idempotency.md) | Stream identity, versioning, atomicity, and exact retry |
| [0004](adr/0004-private-and-integration-events.md) | Separation of private domain events and public integration events |
| [0005](adr/0005-nats-event-store.md) | Per-event JetStream representation and operator-owned policy |
| [0006](adr/0006-messaging-boundaries.md) | Broker-neutral messaging and NATS adapter ownership |
| [0007](adr/0007-legacy-import.md) | Truthful legacy-state import and provenance |
| [0008](adr/0008-nexus-release-strategy.md) | Independent release and thin Nexus integration facade |
| [0009](adr/0009-development-platform-layers.md) | Optional platform layers around a stable explicit kernel |
| [0010](adr/0010-domain-descriptors-registration-and-macros.md) | Shared descriptors and automatic generated registration |
| [0011](adr/0011-inspection-simulation-and-dispatch.md) | Separate inspection, simulation, and live dispatch capabilities |
| [0012](adr/0012-studio-and-ai-control-plane.md) | One secured control plane for Studio and AI tooling |

The product direction can be summarized as follows:

> Zeitstrahl makes a domain model executable, replayable, inspectable, testable,
> visualizable, and understandable by both developers and AI.
