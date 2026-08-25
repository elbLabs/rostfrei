# Zeitstrahl

Zeitstrahl is a Rust event-sourcing and messaging framework. It keeps domain
aggregates independent from persistence, serialization, and brokers while
providing strict execution and NATS JetStream adapters at the application edge.

The initial workspace contains four crates:

- `zeitstrahl-core`: aggregate execution, event-store contracts, and the
  in-memory reference store.
- `zeitstrahl-messaging-core`: transport-neutral commands, integration events,
  queries, envelopes, and delivery contracts.
- `zeitstrahl-nats`: NATS messaging and authoritative JetStream event storage.
- `zeitstrahl-testing`: reusable event-store contracts and aggregate scenarios.

Zeitstrahl does not provision infrastructure during service startup. Operators
use the explicit provisioning APIs with deployment-owned stream policies.

The architectural decisions for the first release are in [`docs/adr`](docs/adr).
