# rostfrei project status and direction

Status as of 2026-08-26.

## Summary

rostfrei is a Rust event-sourcing and messaging framework built around strict
domain boundaries and NATS JetStream. Its working foundation provides typed
aggregate execution, deterministic replay, optimistic concurrency, exact retry,
atomic event commits, transport-neutral messaging, and NATS adapters.

The project is evolving toward an event-sourced development platform in which
the same explicit domain metadata powers runtime dispatch, testing,
visualization, documentation, and AI-assisted development. The aim is not to
hide the existing kernel behind runtime magic. Higher-level tooling will be
generated from inspectable contracts and remain optional.

## Current implementation

The workspace currently contains nine crates:

| Crate | Responsibility |
| --- | --- |
| `rostfrei-core` | Aggregates, execution, event-store contracts, and the in-memory reference store |
| `rostfrei-domain` | Rich domain IDs, descriptors, ownership, model validation and projection, and domain-test metadata |
| `rostfrei-domain-macros` | Derives and attributes for contexts, aggregates, entities, identities, value objects, services, commands, events, errors, actions, decisions, invariants, queries, lifecycles, and domain tests |
| `rostfrei-domain-runtime` | Compile-time checked bindings from model-owned commands to executable rostfrei aggregate handlers |
| `rostfrei-registry` | Stable command descriptors, explicit domain modules, and deterministic validated registration |
| `rostfrei-macros` | Derives that implement command and module metadata contracts |
| `rostfrei-messaging-core` | Transport-neutral commands, integration events, queries, envelopes, and delivery contracts |
| `rostfrei-nats` | NATS messaging and authoritative JetStream event storage |
| `rostfrei-testing` | Aggregate scenarios and reusable event-store contracts |

The aggregate model is deliberately small. Aggregates own typed decisions and
deterministic state transitions. The executor owns load, replay, command
handling, event encoding, append, exact retry detection, and bounded conflict
retry. Aggregate state does not depend on Serde, NATS, clocks, IDs, or storage
handles.

The domain-model layer is now part of rostfrei. It compiles annotated Rust
types into structured metadata for bounded contexts, aggregates, entities,
identities, value objects, services, commands, events, errors, actions,
decisions, invariants, queries, and entity lifecycles. Explicit `domain_model!`
inventories validate cross-references and project the model for tooling.

The runtime bridge maps a descriptive aggregate marker to a rostfrei runtime
aggregate. Its `domain_module!` macro derives command ownership and structural
metadata from `DomainCommandType`, requires a runtime wire name and schema
version, and preserves the rich command descriptor in `DomainRegistry`. The
existing standalone `Command` and `Module` derives remain available for kernel
users that do not adopt the compiled domain model.

rostfrei Studio is implemented as a read-only compiled-model browser and Cargo
diagnostic client. It does not yet provide event timelines, aggregate state
inspection, simulation, runtime command dispatch, or AI APIs. Runtime command
deserialization, erased invocation, automatic discovery, and generated wire
codecs also remain deferred.

The NATS event store writes one JetStream message per domain event. Multi-event
commits use the NATS ADR-50 atomic batch protocol, with one shared batch identity
and the commit header on the final event. This requires NATS Server 2.12.0 or
newer and supports at most 1,000 events in one commit. Retry correctness is a
persisted rostfrei semantic and does not rely on the finite JetStream message
deduplication window.

Messaging supports typed commands, integration events, and queries; bounded
wire envelopes; PubAck-confirmed publication; durable pull consumers; delayed
retry; bounded delivery attempts; quarantine; and Core NATS request/reply.
Infrastructure provisioning remains an explicit operator action rather than a
service-startup side effect.

## Nexus integration

Nexus currently adopts rostfrei as its messaging foundation through a thin
`nexus-messaging` policy facade. an integrating application uses rostfrei publishing,
consumption, retry, quarantine, and topology validation while retaining its
application-owned addresses and deployment defaults.

The integration does not yet run a Nexus aggregate through rostfrei's
`Executor`, `EventStore`, or `NatsEventStore`. No production aggregate is being
converted solely to demonstrate the framework. The existing an integrating application path
remains command to command to a NATS KV entitlement snapshot.

## Verification

The current local implementation has passed workspace tests and strict clippy,
real NATS 2.12 event-store tests, concurrent append tests, the 1,000-event batch
boundary, atomic capacity-failure tests, exact replay contracts, focused Nexus
tests, Nexus architecture tests, and destructive an integrating application NATS tests including
quarantine behavior.

The work is not released. rostfrei has no configured Git remote, recent
changes remain local, and Nexus still uses temporary local path dependencies.
The Nexus release requires an operator-provided Git URL and a pin to one full
rostfrei commit SHA. Database-backed Nexus SQLx tests also require
`DATABASE_URL`.

## Agreed direction

rostfrei will grow in layers around the stable kernel:

1. The implemented domain compiler describes domain structure and behavior
   contracts independently of execution, persistence, and transport.
2. The implemented runtime bridge connects model-owned commands to typed
   aggregate handlers while preserving runtime-only schema versions and names.
3. The implemented registry accepts explicitly registered domain modules and
   exposes deterministic, validated metadata. Automatic linked discovery
   remains deferred.
4. The implemented Studio browses compiled domain structure and Cargo
   diagnostics; runtime inspection views remain deferred.
5. An aggregate inspector will expose redacted developer views without making
   aggregate state a persisted Serde contract.
6. A simulation runtime will replay real history into an isolated branch and
   execute commands without appending or publishing.
7. rostfrei Studio will visualize event timelines, state at any version,
   state differences, operation metadata, command outcomes, and rejections.
8. A protocol-independent control plane will serve both Studio and AI tools with
   the same authorization, redaction, and audit boundaries.

The three operational modes are deliberately distinct:

| Mode | Effect |
| --- | --- |
| Inspect | Read history and reconstruct state without mutation |
| Simulate | Execute against an isolated in-memory branch without publication or append |
| Dispatch | Execute or publish a real command through an explicitly authorized capability |

## Delivery order

1. Complete the standalone rostfrei release and replace Nexus path
   dependencies with a pinned Git revision.
2. Consolidate the absorbed domain model and runtime registry around one typed,
   versioned model contract.
3. Replace runtime model-assembly panics and unversioned JSON with structured
   diagnostics and a versioned projection.
4. Add erased command execution and safe simulation over the registered typed
   contracts without introducing transport concerns.
5. Add aggregate inspection and redaction.
6. Generate command, event, rejection, and inspection schemas.
7. Build the first event timeline and command laboratory in rostfrei Studio.
8. Expose the control plane through MCP and other AI-facing protocols.
9. Add projection management, schema evolution, snapshots, process managers,
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
| [0010](adr/0010-domain-descriptors-registration-and-macros.md) | Shared descriptors and generated registration; the first slice uses explicit modules |
| [0011](adr/0011-inspection-simulation-and-dispatch.md) | Separate inspection, simulation, and live dispatch capabilities |
| [0012](adr/0012-studio-and-ai-control-plane.md) | One secured control plane for Studio and AI tooling |
| [0013](adr/0013-domain-event-handlers.md) | Typed post-commit domain-event handlers and durable NATS dispatch |
| [0014](adr/0014-compiled-domain-model.md) | Absorb the domain compiler as rostfrei's canonical optional platform model |

The product direction can be summarized as follows:

> rostfrei makes a domain model executable, replayable, inspectable, testable,
> visualizable, and understandable by both developers and AI.
