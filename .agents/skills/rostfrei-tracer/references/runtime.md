# Release and execution prerequisites

Install a skills-enabled Rust setup CLI once, then explicitly set up the
application whose dependency is resolved in `Cargo.lock`:

```sh
cargo install --locked --git https://github.com/elbLabs/rostfrei \
  --rev <skills-enabled-sourceRevision> rostfrei-structure
cargo rostfrei skills install
# After upgrading the app's framework dependency/lockfile:
cargo rostfrei skills update
```

Setup follows the actual resolved framework version/revision, acquiring and caching
its matching CLI when needed. The default destination is the selected app's
`.agents/skills`; commit its managed baseline too. Explicit updates preserve
user-added files and reject edited managed-file conflicts before changing any
skill. Merely adding the dependency or starting the app writes no skills.

Initial compilation requires Rust 1.98+, Cargo, Git, a native linker and source
access. Consumer installation/execution needs no Python. PR #106 must be merged
or selected by its reviewed revision; published `v0.0.5-alpha` predates setup
support. Unsupported older dependencies do not receive a newer tool's skills.
`skills info` identifies the invoked tool; installed `references/release.json`
identifies the selected skills. Native execution dispatches to the already-cached
matching tool without Cargo, Git, downloads or source files. Keep
`$CARGO_HOME/rostfrei/tools` (default `$HOME/.cargo/rostfrei/tools`, overridden by
`ROSTFREI_TOOLS_DIR`) available to the execution environment.

In the framework checkout use `cargo rostfrei-dev skills info` and
`cargo rostfrei-dev test <definition.json> --output <new-evidence.json>`. Source
skills need no generated resources or bootstrap. Cargo refreshes the CLI's build
revision after checkout changes; the CLI fetches the actual live schema.

Installed example execution:

```sh
cargo rostfrei test /path/to/rostfrei-develop-feature/assets/add-first-bicycle.json \
  --output behavior-evidence.json
```

For an application-owned definition add
`--release /path/to/rostfrei-tracer/references/release.json`. The CLI checks the
kit/framework version, source revision and protocol versions. Match the
application dependency revision too; the live catalog does not identify a release.

## HTTP, CLI and runtime capabilities

Tracer's implemented adapter is HTTP JSON plus SSE. Use the native Rust CLI for
behavior suites and direct Bash/curl for discovery, recorded operations,
observation and quarantine. There is no MCP
server or native `observe`/`preview` subcommand. MCP is a possible future adapter
of the same service, not an extra capability the agent should assume exists.

Real Test execution and recorded observation are the primary workflows. Legacy
Tracer Preview submission is retired. Optional `simulateHandlerHrefTemplate`
exposes core `CommandExecutor::simulate` using the same registered handler and
dependencies over configured history (Test history in the shared NATS host).
It returns a distinct `handlerSimulation` diagnostic: decision, starting versions
and proposed/uncommitted events, with no receipt or synthetic observed trace.
Dependencies are not automatically sandboxed. It cannot substitute for real Test
acceptance. Never remap a missing/retired capability to Test or production dispatch.

## Contracts

At the specified baseline, Catalog is **v1**, behavior definitions and observed
MessageSeries are **v2**, and fixtures are **v1**. The release changelog explicitly
records discovery changing from v2 to v1; ADR 0037 retains older wording.
Never relabel a live catalog to satisfy an outdated issue description.

Commands address bounded contexts. For example, `add-bicycle` schema 2 contains
`fleet_id`, `bicycle_id` and `condition`. Direct typed HTTP submission wraps it as
`{"schemaVersion":2,"payload":{...}}`. No aggregate ID is added to the command
route. Follow advertised links and actual registered payload versions.

## Real isolated runtime

Use the application's handlers, supervised Test workers, fixtures and NATS event
store on NATS Server 2.12.1+ with JetStream. Simulation is not a substitute for an
unavailable Test/reset capability. Use a disposable broker or uniquely owned
application namespace; never reset unknown shared resources. Run suites serially
until independent Test-run isolation is available.

The framework example's Tracer-enabled startup is
`cargo run --locked -p bike-rental --bin bike-rental-api -- --tracer`, with
`ROSTFREI_NATS_URL` pointing to the disposable broker and distinct
`ROSTFREI_API_TOKEN` / `ROSTFREI_DISPATCH_TOKEN` available in the host environment.
Set optional `ROSTFREI_INSPECTION_TOKEN` on the host and client for read-only
production observation/quarantine. Test inspection uses the control token.
It provisions both Normal and Test topology and resets Test on startup. Default
bind: `127.0.0.1:1309`, configurable via `ROSTFREI_API_ADDR`.
The shared host's ordinary startup without `--tracer` runs the application with
no Tracer HTTP surface. Its `default-run` also allows `cargo run --locked -p
bike-rental -- --tracer`. Older baseline hosts always enabled Tracer and ignored
this flag. Consumer applications supply their own entry point and explicitly
configured observation feeds; discover optional relations in their live catalog.

Reset recreates the configured Test topology, including event history/receipts,
messaging/quarantine streams and consumers, and invalidates Test evidence.
It is not limited to a named fixture or aggregate. External stores/read models
are covered only by explicit application reset support. Production capabilities
remain separate from authorization for a specific isolated TDD loop.
