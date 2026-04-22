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
- `rostfrei-control-plane`: explicitly registered command simulation, bounded
  in-memory operation traces, bounded concurrent admission, status resources,
  and an optional authenticated HTTP/SSE adapter.
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
- `rostfrei-nats`: command and integration-event bus adapters, NATS messaging,
  and authoritative JetStream event storage.
- `rostfrei-testing`: reusable event-store contracts and aggregate scenarios.

Messaging is application-scoped. An application name such as `fast-inbox`
derives its command, integration-event, and quarantine streams and prefixes all
rostfrei business subjects. Bounded contexts derive typed addresses and
authoritative domain-event streams. See
[`docs/messaging-and-nats.md`](docs/messaging-and-nats.md) for the conventions
and provisioning API.

[`examples/bike-rental`](examples/bike-rental) is a self-contained public
example with an aggregate action, a decision, a query, a domain event, and a
domain error. It also contains a runnable local control-plane server that routes
`RentBicycle` through `CommandBus`, executes it against `NatsEventStore`, maps
the committed `BicycleRented` fact to `BicycleRentalStarted`, and publishes that
public event through `IntegrationEventBus`. The same command can be simulated
without mutation. The aggregate identity in the API is qualified by its bounded
context, and `#[domain(json)]` supplies the example's generated command and
rejection JSON while aggregate event JSON comes from the compiled aggregate
codec.

A control-plane instance receives one explicit read-only `EventHistory` and
exposes only commands with matching executable bindings. Live dispatch also
requires an explicit adapter over `CommandBus` and a separately mounted,
separately authorized HTTP router. The control plane translates external JSON
into the bus's dynamic request without defining a second execution or wire path.
When history is a `NatsEventStore`, its configuration fixes the application and
bounded-context stream scope; the context-qualified HTTP route must address that
same history. Operation status and traces are retained only in memory, payloads
are redacted by default, and local deployments must opt in explicitly to expose
them.

rostfrei does not provision infrastructure during service startup. Operators
use the explicit provisioning APIs with bounded, application-scoped defaults
that the deployment can override.
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
