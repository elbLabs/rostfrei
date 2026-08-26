# Rostfrei

Rostfrei is a Rust domain-modeling, event-sourcing, and messaging platform. It
keeps domain aggregates independent from persistence, serialization, and brokers
while providing a compiled domain model, strict execution, developer tooling,
and NATS JetStream adapters at the application edge.

The workspace contains nine crates:

- `rostfrei-core`: aggregate execution, event-store contracts, and the
  in-memory reference store.
- `rostfrei-domain`: the compiled domain model, descriptors, ownership rules,
  model projection, and domain-test metadata.
- `rostfrei-domain-macros`: derives and attributes for domain types and
  behavior contracts.
- `rostfrei-domain-runtime`: bindings from compiled domain commands to
  executable Rostfrei aggregates and runtime registration.
- `rostfrei-registry`: explicit command metadata, domain modules, and
  deterministic runtime registration.
- `rostfrei-macros`: derives for command definitions and domain modules.
- `rostfrei-messaging-core`: transport-neutral commands, integration events,
  queries, envelopes, and delivery contracts.
- `rostfrei-nats`: NATS messaging and authoritative JetStream event storage.
- `rostfrei-testing`: reusable event-store contracts and aggregate scenarios.

[`studio`](studio) contains Rostfrei Studio, a Tauri application for browsing
compiled models and Cargo diagnostics. The domain language reference and
handbook are in [`docs/domain-model`](docs/domain-model).

Rostfrei does not provision infrastructure during service startup. Operators
use the explicit provisioning APIs with deployment-owned stream policies.
Authoritative NATS event storage requires NATS Server 2.12.0 or newer for atomic
multi-event publishing.

The current implementation status, agreed direction, delivery order, and
architecture decision map are in
[`docs/project-status-and-direction.md`](docs/project-status-and-direction.md).
The ADR-derived product overview is in
[`docs/cofounder-project-summary.md`](docs/cofounder-project-summary.md).
The canonical project terminology is in
[`UBIQUITOUS_LANGUAGE.md`](UBIQUITOUS_LANGUAGE.md), and individual decisions are
recorded in [`docs/adr`](docs/adr).
