# rostfrei

<img width="160" height="160" alt="raccoon_smith" src="https://github.com/user-attachments/assets/498043fe-2f24-4ba8-b61e-04c7bb2fbb13" />

rostfrei is a Rust domain-modeling, event-sourcing, and messaging platform. It
keeps domain aggregates independent from persistence, serialization, and brokers
while providing a compiled domain model, strict execution, developer tooling,
and NATS JetStream adapters at the application edge.

The workspace contains fourteen framework crates plus the bike-rental example
Cargo package:

- `rostfrei`: application facade for the compiled domain model, typed command,
  query, and integration-event buses, event-sourcing runtime, registry, and
  public macros.
- `rostfrei-http`: explicitly mounted standard HTTP POST query and command
  routes backed by the shared runtime registry and application buses.
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
- `rostfrei-fixtures`: named, revisioned domain-event MessageSeries validation
  and deterministic typed replay into event stores.
- `rostfrei-structure`: the versioned typed-domain filesystem checker and
  `cargo-rostfrei` command.
- `rostfrei-registry`: handler-linked command metadata, registered query
  metadata, and deterministic runtime registration.
- `rostfrei-macros`: the low-level `QueryDefinition` derive for registered
  application queries.
- `rostfrei-messaging-core`: transport-neutral commands, integration events,
  queries, envelopes, and delivery contracts.
- `rostfrei-nats`: command and integration-event bus adapters, NATS messaging,
  and authoritative JetStream event storage.
- `rostfrei-testing`: reusable event-store contracts and aggregate scenarios.

Messaging is application-scoped. An application name such as `fast-inbox`
derives its command, command-response, integration-event, and quarantine streams
and prefixes all rostfrei business subjects. Bounded contexts derive typed
addresses and authoritative domain-event streams. Normal traffic uses that
canonical namespace; Test automatically inserts a reserved `.test` subject
scope and uses separate derived streams without inventing another application.
See
[`docs/adr`](docs/adr) for the messaging conventions and provisioning decisions.

[`examples/bike-rental`](examples/bike-rental) is a self-contained public
example with rental, return, and fleet-addition commands plus their decisions,
queries, events, and domain errors. Its runnable NATS-backed local Tracer routes
Test and Dispatch through the shared `CommandBus`, executes against
`NatsEventStore`, maps committed domain events to public integration events,
publishes them through `IntegrationEventBus`, and adapts typed integration-command
mappings to deterministic dispatch. Simulate remains read-only, Test
uses resettable isolated state, and Dispatch requires separate production
authorization. The aggregate identity in the API is qualified by its bounded
context. Command and rejection derives supply their canonical JSON codecs,
while aggregate event JSON comes from the compiled aggregate codec.

A Tracer instance receives an explicit test `EventHistory` for discovery,
dynamic inputs, and read-only Simulate. Test and Dispatch instead use separately
configured implementations of the same protocol-neutral command transport. The
bike-rental example instantiates normal and test NATS runtimes for one canonical
`bike-rental` application:
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

The canonical project terminology is in
[`UBIQUITOUS_LANGUAGE.md`](UBIQUITOUS_LANGUAGE.md), and individual architecture
decisions are recorded in [`docs/adr`](docs/adr).

## NATS authentication

`rostfrei_nats::NatsConnectionConfig` supports username/password authentication
for the entire connection, including reconnects and failover:

```rust
use rostfrei_nats::{NatsConnectionConfig, connect};

let config = NatsConnectionConfig::from_server_pool(
    "my-application",
    ["nats://nats-a:4222", "nats://nats-b:4222"],
)
.with_user_and_password(
    std::env::var("NATS_USERNAME")?,
    std::env::var("NATS_PASSWORD")?,
);
let connection = connect(&config).await?;
```

Credentials embedded in URLs (for example,
`nats://app:p%40ss%3Aword@nats-a:4222`) are percent-decoded once; explicit builder
values are used literally. Both username and password must be nonempty. A
URL-only pool must contain the same complete credential pair on every entry,
or no credentials on any entry. With explicit credentials, URLs may omit the
pair, but any embedded pair must be complete and match. Invalid or conflicting
configuration is rejected before connecting.

Token and callback authentication from the managed connection API remain
available. They cannot be combined with username/password credentials embedded
in server URLs; select a single connection-wide authentication method.

Credentials are removed from server addresses before they reach the NATS client
and omitted from configuration debug output and returned errors. The resolved
pair is also used when reconnecting to servers discovered by NATS.

The pinned `async-nats` 0.50.0 dependency logs raw CONNECT fields, including
credentials, at TRACE level. Keep `async_nats::connection` logging below TRACE
when using authentication; Rostfrei's configuration redaction does not filter
the dependency's protocol logs.

## Standard HTTP API

Registered queries accept their JSON payload directly in a POST request:

```sh
curl --request POST \
  --header 'content-type: application/json' \
  --data '{"product_id":"product-1","filters":{"available":true}}' \
  http://127.0.0.1:3000/contexts/catalog/queries/find-product/schemas/1
```

Although POST carries the query payload, registered queries remain safe and
idempotent by application contract: query handlers must not mutate application
state. POST is used so structured and nested query input has one lossless JSON
representation. Query responses use `Cache-Control: private, no-store`.

Commands continue to use their existing POST route, JSON body, and mandatory
`Idempotency-Key` header.

## Macro setup

Crates that declare Rostfrei domain types install macro support once at their
crate root:

```rust
rostfrei::install_macro_support!();
```

The generated crate-local bridge keeps macro expansion deterministic without
adding technical path arguments to individual domain declarations.

## Development

The repository pins Rust 1.98 with Clippy, rustfmt, and `rust-src` through
[`rust-toolchain.toml`](rust-toolchain.toml). Standard-library sources keep the
compile-failure test diagnostics consistent across local machines and CI.

### Tests

With Python 3.11+ and a running local Docker engine, run the complete Rust suite:

```sh
python3 scripts/test_nats.py
```

The default run first fetches the separately locked dependencies needed by the
offline macro compatibility tests. It then starts a pinned NATS 2.12.1 container
with JetStream, fresh storage,
random loopback-only ports, and the required payload limit. It waits for
JetStream readiness, sets bounded bike-rental stream limits, and runs
`cargo test --locked --workspace --all-features -- --test-threads=1`. It removes
its container and storage on success, failure, or cancellation, and prints
broker logs on failure. An inherited `ROSTFREI_NATS_URL` is always replaced with
the disposable broker's address.

To run a narrower suite using the same setup:

```sh
python3 scripts/test_nats.py -- cargo test --locked -p rostfrei-nats -p bike-rental -- --test-threads=1
```

Shared-broker NATS tests are ordinary, non-ignored tests. Direct Cargo runs that include
them require an explicit, nonempty `ROSTFREI_NATS_URL`; missing configuration is
an error, never a successful skip. Use a disposable broker: these tests provision
and remove streams. Run `cargo test --locked -p rostfrei-tracer --features http`
for a broker-free Tracer API check.

The **Tests** GitHub Actions workflow runs the Rust suite with the same broker
runner, and separately runs Studio lint/build, mocked-API browser tests, and
visual inspection. Screenshots, layout state, and browser diagnostics are
uploaded as artifacts. Studio browser tests do not yet exercise a live Tracer
backend. The runner's own lifecycle tests need no Docker:

```sh
python3 -m unittest discover -s scripts -p 'test_*.py'
```

### Git hooks

Enable the tracked Git hooks once per checkout:

```sh
git config core.hooksPath .githooks
```

The pre-commit hook runs the Rostfrei structure checker, then Clippy for every
workspace package, target, and feature:

```sh
cargo rostfrei check --workspace
cargo clippy --workspace --all-targets --all-features
```

`cargo rostfrei check` validates the versioned typed-domain structure configured
by participating packages and executes each package's `rostfrei-domain-check`
target to validate its compiled domain model. A commit is rejected when either
check or Clippy reports an error.

The NATS authentication acceptance suite starts isolated Docker containers and
tests correct, missing, and wrong credentials, percent-encoded URL credentials,
server restart, pool failover, and discovered-server failover:

```sh
cargo test --locked -p rostfrei-nats --test authentication_integration
```

CI also installs a checksum-pinned native NATS server and explicitly runs the
authentication callback, signed-challenge, mutual TLS, and timeout fixtures.
To run those locally, install NATS 2.12.15 and OpenSSL, then use:

```sh
ROSTFREI_NATS_SERVER=/path/to/nats-server \
  cargo test --locked -p rostfrei-nats --test connection_integration -- --include-ignored --test-threads=1
```

### Crate versions and releases

Framework crates share `[workspace.package].version`. Internal dependencies,
including renamed dependencies, inherit their paths and version requirements
from `[workspace.dependencies]`. CI checks this with:

```sh
python3 scripts/versions.py check
```

To prepare a new version, use Python 3.11+ and the repository Rust toolchain:

```sh
python3 scripts/versions.py bump 0.0.5-alpha
```

This updates the root manifest and both Cargo lockfiles, including the standalone
macro dependency-matrix fixture. Cargo runs offline and retains locked registry
versions; dependencies must already be cached. On failure, the command restores
the manifest and lockfiles. Review and commit the resulting diff together.
The fixture packages keep their private `0.0.0` versions.

The **Prepare GitHub release** workflow must run from `main` with a tag matching
the shared version exactly, including the `v` prefix (for example,
`v0.0.5-alpha`). You can check that locally with:

```sh
python3 scripts/versions.py check --tag v0.0.5-alpha
```

The workflow creates a draft GitHub release; it does not publish crates.

## License

Copyright (c) 2026 elbtech.dev.

Licensed under the [European Union Public Licence 1.2](LICENSE).
