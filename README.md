# rostfrei

rostfrei is a Rust domain-modeling, event-sourcing, and messaging platform. It
keeps domain aggregates independent from persistence, serialization, and brokers
while providing a compiled domain model, strict execution, developer tooling,
and NATS JetStream adapters at the application edge.

The workspace contains ten crates:

- `rostfrei`: application facade for the compiled domain model,
  event-sourcing runtime, registry, and public macros.
- `rostfrei-core`: aggregate execution, event-store contracts, and the
  in-memory reference store.
- `rostfrei-domain` (imported as `domain`): the compiled domain model,
  descriptors, ownership rules, model projection, and domain-test metadata.
- `rostfrei-domain-macros`: derives and attributes for domain types and
  behavior contracts.
- `rostfrei-domain-runtime`: stream-aware aggregate initialization, event
  application, and runtime registration for compiled domain types.
- `rostfrei-registry`: explicit command metadata, domain modules, and
  deterministic runtime registration.
- `rostfrei-macros`: derives for command definitions and domain modules.
- `rostfrei-messaging-core`: transport-neutral commands, integration events,
  queries, envelopes, and delivery contracts.
- `rostfrei-nats`: NATS messaging and authoritative JetStream event storage.
- `rostfrei-testing`: reusable event-store contracts and aggregate scenarios.

[`studio`](studio) contains rostfrei Studio, a Tauri application for browsing
compiled models and Cargo diagnostics. The domain language reference and
handbook are in [`docs/domain-model`](docs/domain-model).

Messaging is application-scoped. An application name such as `fast-inbox`
derives its command, integration-event, and quarantine streams and prefixes all
subjects. Bounded contexts derive typed addresses and authoritative domain-event
streams. See [`docs/messaging-and-nats.md`](docs/messaging-and-nats.md) for the
conventions and provisioning API.

[`examples/bike-rental`](examples/bike-rental) is a self-contained public
example with an aggregate action, a decision, a query, a domain event, and a
domain error.

rostfrei does not provision infrastructure during service startup. Operators
use the explicit provisioning APIs with bounded, application-scoped defaults
that the deployment can override.
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

## License

Copyright (c) 2026 elbtech.dev.

Licensed under the [European Union Public Licence 1.2](LICENSE).
