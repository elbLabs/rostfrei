# Rostfrei

**Understand your code. Even when AI wrote it.**

Rostfrei is a Rust framework for domain modeling, event sourcing, and messaging.
It connects business ownership to Rust implementations, event history, and
runtime tools used by developers and coding agents. Macros describe the model;
`cargo rostfrei check` validates its structure. Domain behavior stays independent
of persistence and messaging adapters.

**Status: alpha.** APIs are evolving. Use the runnable example to evaluate the
framework.

## Start here

- [Getting started](https://elblabs.github.io/rostfrei/docs/getting-started):
  inspect the model, start NATS, and open Tracer Studio.
- [Bike-rental example](examples/bike-rental): run a NATS-backed application
  with rental, return, fleet-addition, and cross-fleet transfer commands.
- [Documentation](https://elblabs.github.io/rostfrei/docs): domain macros and
  project structure.
- [Reference index](docs/README.md): workspace crates, terminology, and
  architecture decisions.
- [Runtime stack and checklist](docs/design/command-runtime-stack-and-consolidation.md):
  command execution, persistence, validation, and measured consolidation results.
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

Completed business rejections now share that atomic operation authority across
direct, local, NATS and HTTP execution. Retry a completed operation with unchanged
evidence to replay it; a new attempt after repair needs a new operation ID. Permanent
decisions outlive transport TTL/dedup caches; result details have an explicit expiry
lifecycle, and unreadable completed results never permit reevaluation. Generic executors require an
explicit rejection codec; processor bindings use their existing rejection mapper
before persistence. **NATS defaults to fail-closed legacy/read-only processing.**
New mutable namespaces require an explicit fresh outcome epoch and operator-enforced
old-writer credential/ACL exclusion. Read the
[completed-outcome contract](docs/adr/0044-completed-outcomes-and-envelope-ingress.md)
and [cutover runbook](docs/completed-outcome-cutover.md); do not activate an old
namespace just because its cached responses have expired.
Journal and outcome-state streams form one incarnation-fenced authority pair;
see the [native protocol](docs/design/modular-command-native.md).

For an owned fresh bike-rental application, explicitly set
`ROSTFREI_COMPLETED_OUTCOME_EPOCH` before starting
`cargo run --locked -p bike-rental --bin bike-rental-api -- --tracer`.
The epoch selects split authority for both isolated Test and Normal scopes;
omitting it retains legacy/read-only semantics. See the
[host configuration](docs/nats-application-host.md).

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

Tracer retains operation status and traces in bounded memory. See the
[example guide](examples/bike-rental/README.md#tracer) for setup and behavior.
It also supports [continuous observation](docs/continuous-observation.md) of
externally triggered flows and [quarantine inspection](docs/quarantine-inspection.md).

For explicit rule debugging, **Simulate handler** computes the registered
handler's decision, starting stream versions and proposed/uncommitted events.
It has a distinct result, no observed trace or acceptance receipt, and does not
automatically sandbox injected dependencies. See [the interface and limits](docs/tracer-preview-retirement.md).

For saved query views, [read models](https://elblabs.github.io/rostfrei/docs/read-models)
combine typed event transformations with KV persistence, checkpoints, and
concurrency retries. Queries use a read-only handle.

See the [runtime reference](docs/runtime.md) for NATS authentication, history
reads and audits, command correlation, and worker delivery.

History reads validate local envelopes and commit structure. Deep historical checks
remain available through `audit_history`, `audit_streams`, and
`with_history_auditing(true)`; reads alone do not verify writable authority. See
[read/audit trust boundaries](docs/adr/0040-trusted-event-store-reads-and-explicit-audits.md).

Technical command failures expose typed source and recovery information through
the processor, local/NATS command buses, and HTTP. They are not business
rejections. See the [classification, response compatibility, and redrive contract](docs/adr/0043-typed-technical-command-failures.md)
before upgrading response readers or replaying quarantined commands.

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

## Development

The repository pins Rust 1.98, Clippy, rustfmt, and `rust-src` in
[`rust-toolchain.toml`](rust-toolchain.toml). Enable the tracked pre-commit hook:

```sh
git config core.hooksPath .githooks
```

The hook runs:

```sh
cargo rostfrei check --workspace
cargo clippy --workspace --all-targets --all-features
```

The structure check validates configured domain directories and runs each
participating package's `rostfrei-domain-check` target. For frontend development,
see [website](website/README.md) and [Studio](studio/README.md).

The [development guide](docs/development.md) covers broker-backed tests,
benchmarks, shared crate versions, release preparation, and the starter's pinned
dependency contract.

## License

Copyright (c) 2026 elbtech.dev.

Licensed under the [European Union Public Licence 1.2](LICENSE).
