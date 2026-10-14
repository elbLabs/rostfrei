# Release and execution prerequisites

Use the release-matched Rust CLI. Read the installed skill's
`references/release.json` and install `rostfrei-structure` from that exact revision:

```sh
cargo install --locked --git https://github.com/elbLabs/rostfrei \
  --rev <sourceRevision> rostfrei-structure
cargo rostfrei skills info
```

Initial compilation requires Rust 1.98+, Cargo, Git, a native linker and network
access. Consumer installation/execution needs no Python. After installation the
native `cargo-rostfrei` binary needs neither Git nor a source checkout and does
not contact package registries. The kit contains platform-neutral skills/resources;
Cargo builds the executable for the local platform.

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
store on NATS Server 2.12.1+ with JetStream. Preview is not a substitute for an
unavailable Test/reset capability. Use a disposable broker or uniquely owned
application namespace; never reset unknown shared resources. Run suites serially
until independent Test-run isolation is available.

The baseline example runs with
`cargo run --locked -p bike-rental --bin bike-rental-api`, with
`ROSTFREI_NATS_URL` pointing to the disposable broker and distinct
`ROSTFREI_API_TOKEN` / `ROSTFREI_DISPATCH_TOKEN` available in the host environment.
It provisions both Normal and Test topology and resets Test on startup. Default
bind: `127.0.0.1:1309`, configurable via `ROSTFREI_API_ADDR`.
#107's separate standard host revision uses
`cargo run --locked -p bike-rental -- --tracer`; final combined release and
fresh-session qualification are still integration gates.

Reset recreates the configured Test topology, including event history/receipts,
messaging/quarantine streams and consumers, and invalidates Test/Preview evidence.
It is not limited to a named fixture or aggregate. External stores/read models
are covered only by explicit application reset support. Production capabilities
remain separate from authorization for a specific isolated TDD loop.
