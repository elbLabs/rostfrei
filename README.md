# <img src="packages/ui/src/brand/duck-icon.png" width="36" height="36" alt="" align="absmiddle" /> rostfrei

**Understand your code. Even when AI wrote it.**

Rostfrei is a Rust framework for domain modeling, event sourcing, and messaging.
It connects business ownership to Rust implementations, event history, and
runtime tools used by developers and coding agents. Macros describe the model;
`cargo rostfrei check` validates its structure. Domain behavior stays independent
of persistence and messaging adapters.

**Status: 0.1.** APIs are evolving. Use the runnable example to evaluate the
framework. See the [0.1 migration guide](docs/migration-0.1.md) when upgrading.

## Start here

For the 0.1.2 registry release, add `rostfrei = "0.1.2"` and matching adapters to
your application's dependencies. Install the development CLI separately with
`cargo install --locked --version 0.1.2 rostfrei-structure`; see
[Agent Skills setup](docs/agent-skills-install.md) for dependency-aware installation.

- [Getting started](https://elblabs.github.io/rostfrei/docs/getting-started):
  inspect the model, start NATS, and open Tracer Studio.
- [Bike-rental example](examples/bike-rental): run a NATS-backed application
  with rental, return, fleet-addition, and cross-fleet transfer commands.
- [Official documentation](docs/README.md): shared by developers and agents,
  [published on the website](https://elblabs.github.io/rostfrei/docs) from the same
  source. The [machine index](https://elblabs.github.io/rostfrei/llms.txt) exposes
  the Markdown/MDX pages directly.
- [Command runtime](docs/command-runtime.md): execution, persistence, and recovery.
- [Tracer Studio](studio): inspect behavioral tests and their message graphs.
- [Changelog](CHANGELOG.md): release changes and migration notes.

With Git and [rustup](https://rustup.rs/) installed, inspect the model without a
broker:

```sh
git clone https://github.com/elbLabs/rostfrei.git
cd rostfrei
cargo run --locked -p bike-rental --bin bike-rental-model
```

The repository selects its pinned Rust toolchain automatically.

## Execution model

A command belongs to a bounded context and carries every aggregate ID it needs.
Its handler loads those aggregates and calls domain behavior. The runtime
atomically appends the resulting domain events at the expected stream versions.
A rejection appends no domain events. Accepted commands persist a receipt even
when they emit no events. Queries read state without changing it.

Accepted and rejected decisions are durable across local, NATS, and HTTP execution.
Retry with the original operation ID and evidence to recover the same decision.
Result-detail expiry never permits reevaluation. See the
[command runtime](docs/command-runtime.md) for identity, authorization, and recovery.

Writable NATS startup requires a fresh outcome epoch. The example uses
`ROSTFREI_COMPLETED_OUTCOME_EPOCH`; follow its [setup guide](examples/bike-rental/README.md).
Existing deployments need the [cutover runbook](docs/completed-outcome-cutover.md).

Domain events form private aggregate history. Integration events are separately
versioned public contracts published after commit. NATS JetStream adapters
provide durable command delivery, integration-event messaging, and event storage.
Atomic event transactions require NATS Server 2.12.1 or newer.

Messaging is scoped to one application name, such as `bike-rental`. Isolated
test traffic uses separate subjects and streams under that same application.
Infrastructure is provisioned through explicit APIs; the example calls them
during local startup. See [subjects and streams](https://elblabs.github.io/rostfrei/docs/messaging/subjects)
for the routing conventions.

Tracer exposes two real command actions:

| Action | Effect |
| --- | --- |
| Test | Execute through the command pipeline against isolated test state. |
| Production dispatch | Execute against the production target with a separate capability token. |

Tracer also supports [continuous observation](docs/continuous-observation.md),
[quarantine inspection](docs/quarantine-inspection.md), and uncommitted
[handler diagnostics](docs/handler-simulation.md).

For saved query views, [read models](https://elblabs.github.io/rostfrei/docs/read-models)
combine typed event transformations with KV persistence, checkpoints, and
concurrency retries. Queries use a read-only handle.

See the [runtime reference](docs/runtime.md) for NATS authentication, history
reads and audits, command correlation, and worker delivery.

## HTTP API

Applications explicitly mount the routes provided by `rostfrei-http`:

```text
POST /contexts/{context}/commands/{command}/schemas/{schema_version}
POST /contexts/{context}/queries/{query}/schemas/{schema_version}
```

Both accept the registered JSON payload directly. Commands require an
`Idempotency-Key` header. Queries use POST for structured input; their handlers
must remain read-only. Query responses use `Cache-Control: private, no-store`.

## Macro setup

Install macro support once at the root of each crate declaring domain types:

```rust
rostfrei::install_macro_support!();
```

## Agent Skills

- **`rostfrei-develop-feature`** guides Given/When/Then and test-first development
  through the application's real NATS-backed behavior suite.
- **`rostfrei-tracer`** discovers runtime capabilities and explains tests,
  recorded operations, observations and quarantine.

The [Agent Skills guide](docs/agent-skills-install.md) covers dependency-matched
installation, updates, and application test execution.

## Development

The repository pins Rust 1.98, Clippy, rustfmt, and `rust-src` in
[`rust-toolchain.toml`](rust-toolchain.toml). Enable the tracked pre-commit hook:

```sh
git config core.hooksPath .githooks
```

The hook runs the workspace structure check and Clippy.

The website and Tracer Studio share the Reference ’15 theme and components in
[`@rostfrei/ui`](packages/ui/README.md). Use Node.js 24 (or 22.13+ on the 22.x line) and
pnpm 10.10.0. Install the frontend workspace from this repository root:

```sh
pnpm install --frozen-lockfile
pnpm dev:website
# In another terminal, when working on Studio:
pnpm dev:studio
```

`pnpm build` builds both apps; `pnpm lint` checks all frontend packages.
`pnpm dev:ui` opens the shared component gallery on port 4177.

The [development guide](docs/development.md) covers broker-backed tests,
benchmarks, shared crate versions, release preparation, and the starter's pinned
dependency contract.

## License

Copyright (c) 2026 elbtech.dev.

Licensed under the [European Union Public Licence 1.2](LICENSE).
