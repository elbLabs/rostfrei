# Rostfrei

**Understand your code. Even when AI wrote it.**

<img width="160" height="160" alt="Rostfrei's raccoon blacksmith mascot" src="https://github.com/user-attachments/assets/498043fe-2f24-4ba8-b61e-04c7bb2fbb13" />

Rostfrei is a Rust framework for domain modeling, event sourcing, and messaging.
It helps humans and AI agents work from a shared model of the business: explicit
ownership, typed contracts, event history, and behavior you can inspect. A compiled
domain model connects your declarations to the runtime and developer tools while
keeping persistence, serialization, and brokers outside your aggregates.

[Website](https://elblabs.github.io/rostfrei/) ·
[Documentation](https://elblabs.github.io/rostfrei/docs) ·
[Getting started](https://elblabs.github.io/rostfrei/docs/getting-started) ·
[Bike-rental example](examples/bike-rental) ·
[Changelog](CHANGELOG.md)

**Status: alpha.** APIs are evolving. The runnable example and architecture
decisions are the best starting points for evaluating the framework.

## Why Rostfrei?

An agent can implement a feature. Your team still needs to understand where its
rules live, what they can change, and how the system behaves. Rostfrei connects
the code's structure to its business meaning and runtime evidence:

- **Model the business.** Give commands, policies, and invariants explicit
  owners. Typed declarations and checked project structure help both developers
  and coding agents navigate the model and find the implementation of a rule.
- **Keep the history.** Rebuild aggregate state from domain events. Execute
  commands with atomic commits, expected stream versions, and exact retries,
  including transactions across aggregate streams in the same event store.
- **Connect the application.** Use typed command, query, and integration-event
  buses, with NATS JetStream adapters for durable messaging and event storage.
  Public integration contracts stay separate from private aggregate history.
- **Inspect the behavior.** Use Tracer for read-only Preview, isolated Test
  publication, and behavioral tests. Explore commands and message flows visually
  in [Tracer Studio](studio), or through Tracer's agent-facing HTTP API.

Rostfrei is aimed at applications where business rules, consistency boundaries,
and the history of change are central to the design—especially when teams want
to keep an agent-assisted codebase understandable as it grows.

## Start with a bicycle rental

In the example, `RentBicycle` asks the fleet to make a decision. An accepted
rental commits a private `BicycleRented` domain event. Application code then maps
it to the public `BicycleRentalStarted` integration event. An unavailable bicycle
produces a rejection and no new domain events.

With Git and [rustup](https://rustup.rs/) installed, inspect that model without a
broker:

```sh
git clone https://github.com/elbLabs/rostfrei.git
cd rostfrei
cargo run --locked -p bike-rental --bin bike-rental-model
```

The repository selects its pinned Rust toolchain automatically through rustup.
The command prints the compiled domain model. Continue with the
[getting-started guide](https://elblabs.github.io/rostfrei/docs/getting-started)
to run the NATS-backed application and explore its behavior in Studio.

## Find your way

| Goal | Guide |
| --- | --- |
| Understand the model and its vocabulary | [Documentation introduction](https://elblabs.github.io/rostfrei/docs) and [ubiquitous language](UBIQUITOUS_LANGUAGE.md) |
| Follow a command through NATS | [Subjects and streams](https://elblabs.github.io/rostfrei/docs/messaging/subjects) |
| Declare domain types and behavior | [Domain macros](https://elblabs.github.io/rostfrei/docs/domain-macros) |
| Organize a domain and check ownership | [Project structure](https://elblabs.github.io/rostfrei/docs/project-structure) |
| Understand architectural tradeoffs | [Architecture decisions](docs/adr) |
| Contribute or run the test suite | [Development](#development) |

## Workspace components

The workspace contains fourteen framework crates plus the bike-rental example:

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

## Runtime and messaging

Messaging is application-scoped. An application name such as `fast-inbox`
derives its command, command-response, integration-event, and quarantine streams
and prefixes all rostfrei business subjects. Bounded contexts derive typed
addresses and authoritative domain-event streams. Normal traffic uses that
canonical namespace; Test automatically inserts a reserved `.test` subject
scope and uses separate derived streams without inventing another application.
See
[`docs/adr`](docs/adr) for the messaging conventions and provisioning decisions.
For a visual walkthrough using the bike-rental example, read
[Subjects and streams](https://elblabs.github.io/rostfrei/docs/messaging/subjects)
on the docs website, or open [`docs/subjects.html`](docs/subjects.html) for the
standalone offline guide.

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

The example also includes a self-checking [quarantine walkthrough](examples/bike-rental/QUARANTINE.md)
covering retry exhaustion, quarantine inspection, repair and republication, and
invalid-message handling against real NATS JetStream.

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

Successful command execution always persists an acceptance receipt, including
commands that emit no domain events or load no aggregates. Event-free acceptance
returns `CommandReceipt::AcceptedNoEvents`; retries return `ExactReplay` with an
empty event list. Loaded read-only aggregates are guarded atomically with the
receipt, and changed command content or provenance conflicts under the accepted
operation identity. See [Durable event-free acceptance](docs/adr/0039-durable-event-free-acceptance.md)
for concurrency, storage compatibility, and migration from transient no-op results.

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

## Command execution metadata

`CommandExecutor::execute` and `CommandBus` share the correlation rule
`correlation_id = supplied_correlation_id ?? operation_id`. Direct execution
establishes this default before invoking a handler, checking an existing receipt,
or persisting events. Every committed event and its transaction receipt carry the
same correlation. Retrying the same operation retains that identity; a follow-up
operation can supply the earlier operation's correlation to continue its flow.

Supplied causation is preserved independently. Direct execution leaves omitted
causation absent; the bus processor uses the command message ID when no causation
was supplied. `CommandExecutor::simulate` applies the same correlation default to
the metadata visible to its handler. An operation ID used as the default must
satisfy `CorrelationId` validation; invalid defaults fail before execution.

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

Check that a new application builds outside this checkout, using the exact public
dependencies delivered by the generator (GitHub and crates.io access required):

```sh
python3 scripts/check_new_project.py
```

This generates a hyphenated starter in a temporary directory, runs `cargo check`
and `cargo test --locked` without changing its manifest, and verifies that all
Rostfrei crates resolve from the pinned Git release. The **Tests** workflow runs
this acceptance check on every PR and `main` push; it needs no broker.

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
server restart, pool failover, and discovered-server failover. Its authenticated
JetStream scenarios also exercise command delivery and durable responses, then
recreate the managed connection to verify aggregate/command replay and durable
post-commit consumption with both explicit and URL credentials:

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
python3 scripts/versions.py bump 0.0.6-alpha
```

This updates the root manifest and both Cargo lockfiles, including the standalone
macro dependency-matrix fixture. Cargo runs offline and retains locked registry
versions; dependencies must already be cached. On failure, the command restores
the manifest and lockfiles. Review and commit the resulting diff together.
The fixture packages keep their private `0.0.0` versions.

The **Prepare GitHub release** workflow must run from `main` with a tag matching
the shared version exactly, including the `v` prefix (for example,
`v0.0.6-alpha`). You can check that locally with:

```sh
python3 scripts/versions.py check --tag v0.0.6-alpha
```

The workflow creates a draft GitHub release; it does not publish crates.

### Starter dependency contract

`cargo rostfrei new` pins both `rostfrei` and `rostfrei-nats` to the same full Git
commit from an available GitHub release while the crates are unpublished. The
current starter targets `v0.0.5-alpha` (`c9f5516ddc324265916b60e1eb7300dad9acac9d`).
This pin is intentionally independent of the generator's package version and
`main`: a newer CLI can generate a starter using an older, tested release.
Existing generated projects retain their pin; commit their `Cargo.lock` too.

`scripts/versions.py bump` only bumps the shared workspace versions and lockfiles;
it does **not** advance the starter pin. Thus version-bump PRs and their external
acceptance checks can pass before the new tag exists. Keep templates compatible
with the selected release. After publishing a release, advance `SCAFFOLD_RELEASE`
and `SCAFFOLD_REVISION` in `crates/rostfrei-structure/src/scaffold.rs`, update the
pin regression expectation and this documentation, and run the external acceptance
check in a follow-up PR. Template changes requiring new runtime APIs must wait for
that available release and pin update. Do not point the starter at an unpublished
future tag or silently fall back to a local checkout.

To generate from this checkout:

```sh
cargo rostfrei-dev new ../my-application
cd ../my-application
cargo check
cargo test
```

Names `model` and `tests` are reserved for generated domain modules. Choose a
different name such as `model-app` or `tests-app`. The generated README describes
the release pin, broker-free checks, and Docker Compose startup.

## License

Copyright (c) 2026 elbtech.dev.

Licensed under the [European Union Public Licence 1.2](LICENSE).
