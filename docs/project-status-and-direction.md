# rostfrei project status and direction

Status as of 2026-08-26.

## Summary

rostfrei is a Rust event-sourcing and messaging framework built around strict
domain boundaries and NATS JetStream. Its working foundation provides typed
aggregate execution, deterministic replay, optimistic concurrency, exact retry,
atomic event commits, transport-neutral messaging, and NATS adapters.

The project is evolving toward an event-sourced development platform in which
the same explicit domain metadata powers runtime dispatch, testing, inspection,
documentation, and AI-assisted development. The aim is not to
hide the existing kernel behind runtime magic. Higher-level tooling will be
generated from inspectable contracts and remain optional.

## Current implementation

The workspace currently contains eleven framework crates plus the bike-rental
example Cargo package:

| Crate | Responsibility |
| --- | --- |
| `rostfrei` | Application facade for the compiled domain model, event-sourcing runtime, registry, and public macros |
| `rostfrei-control-plane` | Explicit command simulation bindings, operation status and trace contracts, and an optional HTTP/SSE adapter |
| `rostfrei-core` | Aggregates, execution, event-store contracts, and the in-memory reference store |
| `rostfrei-domain` | Rich domain IDs, descriptors, ownership, model validation and projection, and domain-test metadata |
| `rostfrei-domain-macros` | Derives and attributes for contexts, aggregates, entities, identities, value objects, services, commands, events, errors, actions, decisions, invariants, queries, lifecycles, and domain tests |
| `rostfrei-domain-runtime` | `Apply`, stream-aware initialization, and compile-time checked bindings from compiled aggregates and model-owned commands to the core runtime |
| `rostfrei-registry` | Stable command descriptors, explicit domain modules, and deterministic validated registration |
| `rostfrei-macros` | Low-level derives for standalone command and module metadata contracts |
| `rostfrei-messaging-core` | Transport-neutral commands, integration events, queries, envelopes, and delivery contracts |
| `rostfrei-nats` | NATS messaging and authoritative JetStream event storage |
| `rostfrei-testing` | Aggregate scenarios and reusable event-store contracts |

The aggregate model is deliberately small. A compiled aggregate marker owns its
typed root state and attached concrete event set. Initialization explicitly
receives the stream identity; concrete `Apply<Event>` implementations define
deterministic transitions. Recording applies immediately to live state. The
executor owns load, replay, command handling, event encoding, append, exact
retry detection, and bounded conflict retry. Aggregate state does not depend on
Serde, NATS, clocks, or storage handles.
Command execution returns an accepted receipt or a modeled rejection as its
business outcome; only EventStore and codec failures occupy the execution error
channel. Accepted receipts distinguish appended events, exact replay, and
accepted no-event decisions.

The domain-model layer is now part of rostfrei. It compiles annotated Rust
types into structured metadata for bounded contexts, aggregates, entities,
identities, value objects, services, commands, events, errors, actions,
decisions, invariants, queries, and entity lifecycles. Explicit `domain_model!`
inventories validate cross-references and project the model for tooling. Events
are projected from each aggregate's `events = [...]` attachment rather than a
second flat inventory.

The canonical `Aggregate` derive generates the core aggregate definition, a
doc-hidden aggregate event representation, concrete event conversions,
root `Apply` dispatch, and default JSON codec behavior. `Executor::new(store)`
needs no codec. Unknown event types, unsupported versions, and malformed JSON
fail replay closed; custom `EventCodec` implementations remain explicit
overrides. `DomainCommand` derives runtime command ownership, local command name,
schema version, and structural metadata. Registering an executable command
binding inserts that descriptor into `DomainRegistry`; `domain_module!` is an
optional grouping mechanism rather than a prerequisite. Runtime command identity
is aggregate type, local command name, and schema version, so different
aggregates may use the same local name. Standalone `Command` and `Module` derives
remain available for direct kernel users.

The first headless control-plane slice provides explicitly registered command
deserialization and erased simulation over normal typed aggregate handlers. It
exposes asynchronous operation resources and resumable SSE traces for replay,
command acceptance or rejection, predicted domain events, and completion.
Simulation depends only on read-only event history and never appends or
publishes. The optional HTTP adapter requires a bearer capability, and trace
payloads are redacted unless a deployment explicitly supplies another policy.
The included journal and concurrent simulation admission are bounded and
in-memory; retention is pressure-based and idempotency lasts only while an
operation remains retained. Durable traces, production-grade authorization,
automatic discovery, generated wire schemas, live dispatch, and inspection views
remain deferred.
Default JSON domain-event codecs are implemented by the compiled aggregate
contract. Commands and domain errors can opt into generated conventional JSON
payloads with `#[domain(json)]`; custom command codecs remain explicit overrides.

The NATS event store writes one JetStream message per domain event. Multi-event
commits use the NATS ADR-50 atomic batch protocol, with one shared batch identity
and the commit header on the final event. This requires NATS Server 2.12.0 or
newer and supports at most 1,000 events in one commit. Retry correctness is a
persisted rostfrei semantic and does not rely on the finite JetStream message
deduplication window.

Messaging supports typed commands, integration events, and queries; bounded
wire envelopes; PubAck-confirmed publication; durable pull consumers; delayed
retry; bounded delivery attempts; quarantine; and Core NATS request/reply.
Query requesters expose `QueryResult<T>`, keeping transport and protocol failures
outside `QueryResponse<T>` while application query errors remain response
outcomes.
Application-first addresses and application-derived stream topology make one
validated application name the normal configuration boundary. Bounded contexts
derive authoritative domain-event stream names, subjects, and persisted scope
metadata.
Infrastructure provisioning remains an explicit operator action rather than a
service-startup side effect.

## Nexus integration

An integrating application currently adopts rostfrei as its messaging
foundation through a thin policy facade. ADR 0015 narrows that facade: rostfrei
owns normal address conventions and topology defaults, while the application
supplies its name, business message names, environment variables, deployment
overrides, and operator composition.

The integration does not yet run a production aggregate through rostfrei's
`Executor`, `EventStore`, or `NatsEventStore`. No production aggregate is being
converted solely to demonstrate the framework.

## Verification

The verification suite includes workspace tests and strict Clippy, concurrent
append tests, the 1,000-event batch boundary, atomic capacity-failure tests,
exact replay contracts, and destructive NATS tests including quarantine
behavior. The control-plane slice also has end-to-end tests for
accepted and rejected simulations, idempotency conflicts, resumable SSE traces,
terminal cursors, bearer authorization, default payload redaction, terminal
eviction, descriptor matching, request limits, and unchanged aggregate history.
Real-server tests require `ROSTFREI_NATS_URL`; when it is absent they are
reported as environment-dependent skips rather than successful NATS runs.

The work is not tagged or released. rostfrei has a configured Git remote; Nexus
still uses temporary local path dependencies and must pin one reviewed full
rostfrei commit SHA during its integration task. Database-backed Nexus SQLx
tests also require `DATABASE_URL`.

## Agreed direction

rostfrei will grow in layers around the stable kernel:

1. The implemented domain compiler describes domain structure and behavior
   contracts independently of persistence and transport, while aggregate event
   attachments also generate the optional event-sourcing runtime contract.
2. The implemented runtime bridge connects compiled aggregate roots, concrete
   events, and model-owned commands to typed core execution. Domain-event schema
   versions belong to their canonical definitions.
3. The implemented registry accepts explicitly registered domain modules and
   exposes deterministic, validated metadata. Automatic linked discovery
   remains deferred.
4. An aggregate inspector will expose redacted developer views without making
   aggregate state a persisted Serde contract.
5. The first simulation runtime slice replays real history into an isolated
   branch and executes commands without appending or publishing.
6. A protocol-independent control plane will serve AI tools with the same
   authorization, redaction, and audit boundaries.

The three operational modes are deliberately distinct:

| Mode | Effect |
| --- | --- |
| Inspect | Read history and reconstruct state without mutation |
| Simulate | Execute against an isolated in-memory branch without publication or append |
| Dispatch | Execute or publish a real command through an explicitly authorized capability |

## Delivery order

1. Complete the standalone rostfrei release and replace Nexus path
   dependencies with a pinned Git revision.
2. Extend the consolidated compiled aggregate/runtime contract into erased
   command registration and versioned model compatibility checks.
3. Replace runtime model-assembly panics and unversioned JSON with structured
   diagnostics and a versioned projection.
4. Mature the implemented erased simulation slice with durable operation traces,
   generated wire schemas, and production authorization without introducing
   transport concerns into the kernel.
5. Add aggregate inspection and redaction.
6. Generate command, event, rejection, and inspection schemas.
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
| [0010](adr/0010-domain-descriptors-registration-and-macros.md) | Shared descriptors and generated registration; the first slice uses explicit modules |
| [0011](adr/0011-inspection-simulation-and-dispatch.md) | Separate inspection, simulation, and live dispatch capabilities |
| [0012](adr/0012-ai-control-plane.md) | AI adapters use the secured control plane |
| [0013](adr/0013-domain-event-handlers.md) | Typed post-commit domain-event handlers and durable NATS dispatch |
| [0014](adr/0014-compiled-domain-model.md) | Absorb the domain compiler as rostfrei's canonical optional platform model |
| [0015](adr/0015-application-scoped-nats-conventions.md) | Derive application-first subjects, stream topology, and bounded-context event stores |

The product direction can be summarized as follows:

> rostfrei makes a domain model executable, replayable, inspectable, testable,
> explainable, and understandable by both developers and AI.
