# rostfrei

<img width="160" height="160" alt="raccoon_smith" src="https://github.com/user-attachments/assets/498043fe-2f24-4ba8-b61e-04c7bb2fbb13" />

rostfrei is a Rust domain-modeling, event-sourcing, and messaging platform. It
keeps domain aggregates independent from persistence, serialization, and brokers
while providing a compiled domain model, strict execution, developer tooling,
and NATS JetStream adapters at the application edge.

The workspace contains eleven framework crates plus the bike-rental example
Cargo package:

- `rostfrei`: application facade for the compiled domain model, typed command
  and integration-event buses, event-sourcing runtime, registry, and public
  macros.
- `rostfrei-tracer`: explicitly registered read-only simulation,
  isolated test execution, separately authorized production dispatch, bounded
  in-memory operation traces, and an optional authenticated HTTP/SSE adapter.
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
- `rostfrei-macros`: low-level `CommandDefinition` and `Module` derives for
  direct registry and kernel users.
- `rostfrei-messaging-core`: transport-neutral commands, integration events,
  queries, envelopes, and delivery contracts.
- `rostfrei-nats`: command and integration-event bus adapters, NATS messaging,
  and authoritative JetStream event storage.
- `rostfrei-testing`: reusable event-store contracts and aggregate scenarios.

Messaging is application-scoped. An application name such as `fast-inbox`
derives its command, command-response, integration-event, and quarantine streams
and prefixes all rostfrei business subjects. Bounded contexts derive typed
addresses and authoritative domain-event streams. See
[`docs/adr`](docs/adr) for the messaging conventions and provisioning decisions.

[`examples/bike-rental`](examples/bike-rental) is a self-contained public
example with rental, return, and fleet-addition commands plus their decisions,
queries, events, and domain errors. Its runnable NATS-backed local Tracer routes
Test and Dispatch through the shared `CommandBus`, executes against
`NatsEventStore`, maps committed domain events to public integration events,
and publishes them through `IntegrationEventBus`. Simulate remains read-only,
Test uses resettable isolated state, and Dispatch requires separate production
authorization. The aggregate identity in the API is qualified by its bounded
context, and `#[domain(json)]` supplies the generated command and rejection JSON
while aggregate event JSON comes from the compiled aggregate codec.

[`studio`](studio) is the standalone local UI for catalog discovery, Simulate,
isolated Test, production Dispatch, operation status, and correlation streams.

A Tracer instance receives an explicit test `EventHistory` for discovery,
dynamic inputs, and read-only Simulate. Test and Dispatch instead use separately
configured implementations of the same protocol-neutral command transport. The
bike-rental example instantiates one NATS runtime definition for each environment:
both publish a durable command, execute through a command worker and `Executor`,
append to the environment's `NatsEventStore`, publish a durable accepted or
rejected response, and run the same post-commit domain and integration-event
handlers. Transported submissions require an idempotency key. Test reset rotates
the scenario generation and recreates only the isolated Test topology; a failed
reset keeps Test and its state-dependent discovery unavailable until a reset
succeeds. Operation status and traces are retained only in bounded memory,
payloads are redacted by default, and local deployments must opt in explicitly
to expose them.

rostfrei does not implicitly provision infrastructure. Operators use explicit
provisioning APIs with bounded, application-scoped defaults; the local
bike-rental example invokes them during startup for demonstration.
Authoritative NATS event storage requires NATS Server 2.12.1 or newer. In
addition to atomic multi-event commits, one event transaction can atomically
append commits to multiple aggregate streams in the same bounded-context event
store.

The current implementation status, agreed direction, delivery order, and
architecture decision map are in
[`docs/project-status-and-direction.md`](docs/project-status-and-direction.md).
The ADR-derived product overview is in
[`docs/cofounder-project-summary.md`](docs/cofounder-project-summary.md).
The canonical project terminology is in
[`UBIQUITOUS_LANGUAGE.md`](UBIQUITOUS_LANGUAGE.md), and individual decisions are
recorded in [`docs/adr`](docs/adr).

## Development

The repository pins Rust 1.98 with Clippy and rustfmt through
[`rust-toolchain.toml`](rust-toolchain.toml).

Enable the tracked Git hooks once per checkout:

```sh
git config core.hooksPath .githooks
```

The pre-commit hook runs Clippy for every workspace package, target, and feature:

```sh
cargo clippy --workspace --all-targets --all-features
```

A commit is rejected when Clippy reports an error.

## License

Copyright (c) 2026 elbtech.dev.

Licensed under the [European Union Public Licence 1.2](LICENSE).
