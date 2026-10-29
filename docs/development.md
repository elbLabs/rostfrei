# Development and releases

Run these commands from the repository root. The pinned Rust toolchain includes
Clippy, rustfmt, and `rust-src`; standard-library sources keep compile-failure
diagnostics consistent across local machines and CI.

## Tests

With Python 3.11+ and a running Docker engine, run the Rust workspace suite:

```sh
python3 scripts/test_nats.py
```

The runner fetches separately locked dependencies for offline macro tests,
starts a disposable NATS 2.12.15 container, waits for JetStream readiness, and
runs `cargo test --locked --workspace --all-features -- --test-threads=1`.
It uses fresh storage, random loopback ports, and bounded bike-rental stream
limits. It replaces any inherited `ROSTFREI_NATS_URL`, removes its resources on
exit, and prints broker logs on failure.

To run a narrower suite:

```sh
python3 scripts/test_nats.py -- cargo test --locked -p rostfrei-nats -p bike-rental -- --test-threads=1
```

Shared-broker tests are non-ignored. Direct Cargo runs that include them require
an explicit, nonempty `ROSTFREI_NATS_URL`; missing configuration is an error.
Use a disposable broker because the tests provision and remove streams.

Broker-free checks include:

```sh
cargo test --locked -p rostfrei-tracer --features http
python3 -m unittest discover -s scripts -p 'test_*.py'
```

Check that a generated application builds outside this checkout:

```sh
python3 scripts/check_new_project.py
```

This requires GitHub and crates.io access. It generates a hyphenated starter in
a temporary directory, runs `cargo check` and `cargo test --locked` with its
unmodified manifest, and verifies that Rostfrei dependencies resolve from the
pinned Git release.

### NATS authentication and workers

The authentication acceptance suite starts its own Docker containers. It covers
credentials, reconnects, failover, command delivery, durable responses, replay,
and post-commit consumption:

```sh
cargo test --locked -p rostfrei-nats --test authentication_integration
```

For callback, signed-challenge, mutual TLS, and timeout fixtures, install NATS
2.12.15 and OpenSSL, then run:

```sh
ROSTFREI_NATS_SERVER=/path/to/nats-server \
  cargo test --locked -p rostfrei-nats --test connection_integration -- --include-ignored --test-threads=1
```

Run the [domain-event worker pool](domain-event-workers.md) checks with:

```sh
python3 scripts/test_nats.py -- cargo test --locked -p rostfrei-nats --test domain_event_consumer_integration
```

### Frontends and CI

The frontends use one pnpm workspace and root `pnpm-lock.yaml`. Install with
`pnpm install --frozen-lockfile` from the repository root. The internal
[`@rostfrei/ui`](../packages/ui/README.md) package supplies Reference ’15 tokens,
shadcn/Radix controls, theme selection, and duck branding to both apps. Its source
is consumed by Vite directly; it does not require a separate publish/build step.

```sh
pnpm lint
pnpm build
pnpm test:docs
pnpm test:website
pnpm test:studio
```

See [website development](../website/README.md) and
[Studio development](../studio/README.md). Run `pnpm dev:ui` for the local gallery
in `packages/ui/gallery`, which shows shared components and their interactive states. The **Tests**
workflow runs Rust checks, the standalone starter acceptance check, shared UI
lint/typechecking, website lint, a deployment-base build and browser checks, and Studio
lint/build, mocked-API browser tests, and visual inspection. Screenshots, layout
state, and browser diagnostics are uploaded as artifacts. Browser fixtures do
not exercise a live Tracer backend. Both browser checks use Chrome; set
`CHROME_BIN` if it is not installed in a standard location. Website captures go
to `artifacts/website/`. Set `VITE_BASE_PATH` to the same value for the website
build and its browser check when verifying a non-root deployment.

## Benchmarks

```sh
cargo build --locked --release -p rostfrei-benchmarks --bins
target/release/rostfrei-benchmarks aggregate-loading -- \
  --events 0,100,1000 --samples 5 \
  --output target/benchmarks/aggregate-loading.json
```

The release-mode suite measures history loading, event application, complete
rehydration, and executor commands. It creates direct-appended and
command-generated histories, verifies state, and writes raw samples and summaries
as JSON. Fixture setup is excluded from sample timings. See the
[methodology](benchmarks/aggregate-loading.md) and
[benchmark package](../crates/rostfrei-benchmarks/README.md) for read-model and
paired-comparison workloads. Benchmarking requires Rust and Docker, not Python.

## Crate versions and releases

Framework crates share `[workspace.package].version`. Internal dependencies,
including renamed dependencies, inherit paths and version requirements from
`[workspace.dependencies]`. Check this with:

```sh
python3 scripts/versions.py check
```

To prepare a version:

```sh
python3 scripts/versions.py bump 0.1.2
```

The script updates the root manifest and both Cargo lockfiles, including the
standalone macro dependency-matrix fixture. Cargo runs offline and retains locked
registry versions, so dependencies must be cached. Failure restores the manifest
and lockfiles. Review and commit the diff together; private fixture packages
retain version `0.0.0`.

The **Prepare GitHub release** workflow runs from `main` with a tag matching the
shared version, including the `v` prefix. Validate it locally with:

```sh
python3 scripts/versions.py check --tag v0.1.2
```

The workflow creates a draft GitHub release; it does not publish crates.

### crates.io publication

From 0.1.1, all 14 public library/tooling packages have registry metadata. The
benchmark and bike-rental packages are explicitly `publish = false`. Freeze one
clean commit for the Git tag, Skills kit, CLI and registry packages; dependency-aware
Skills setup checks the published package's `.cargo_vcs_info.json` revision.

With the repository's pinned Cargo version, qualify the complete package graph:

```sh
cargo publish --dry-run --locked --workspace \
  --exclude rostfrei-benchmarks --exclude bike-rental --all-features
```

Cargo packages the selected crates into a temporary registry, verifies their
extracted contents and determines dependency order. Inspect package metadata and
archive contents, retain checksums/provenance, and require exact-source CI and
matching Skills qualification before uploading. Authenticate locally with
`cargo login`, then publish that same clean source:

```sh
cargo publish --locked --workspace \
  --exclude rostfrei-benchmarks --exclude bike-rental --all-features
```

If publication stops partway through, inspect crates.io/index state and archive
checksums before continuing with only the unpublished packages. Do not assume a
failed upload means the server rejected it. After publication, check every version
and checksum and build an external application using registry-only dependencies.
Install the registry CLI and qualify its dependency-aware Skills setup as well.

Registry consumers use `rostfrei = "0.1.2"` and matching adapters, and install the
CLI with `cargo install --locked --version 0.1.2 rostfrei-structure`. The GitHub
workflow remains a separate publication step for the matching kit and checksum.

## Starter dependency contract

`cargo rostfrei new` pins `rostfrei` and
`rostfrei-nats` to the same full Git commit from an available release. The
starter currently targets `v0.0.5-alpha`
(`c9f5516ddc324265916b60e1eb7300dad9acac9d`). This pin is independent of the
generator's package version and `main`. Existing projects retain their pin and
should commit `Cargo.lock`.

`scripts/versions.py bump` does not advance the starter pin. After publishing a
release, update `SCAFFOLD_RELEASE` and `SCAFFOLD_REVISION` in
`crates/rostfrei-structure/src/scaffold.rs`, the pin regression expectation, and
this guide. Run the external acceptance check in that follow-up PR. Templates
must remain compatible with the selected release; new runtime APIs require an
available release and a pin update, not a future tag or local fallback.

The current pin predates bundled Agent Skills setup. `new` reports that skills
were not installed; see the [Agent Skills guide](agent-skills-install.md) for setup,
source commands and kit checks. After publishing a skills-enabled release, qualify
its matching resources and enable `scaffold_dependency()`'s skills capability
alongside the starter pin update.

To generate from this checkout:

```sh
cargo rostfrei-dev new ../my-application
cd ../my-application
cargo check
cargo test
```

`model` and `tests` are reserved domain-module names; use names such as
`model-app` or `tests-app`. The generated README documents its release pin,
broker-free checks, and Docker Compose startup.
