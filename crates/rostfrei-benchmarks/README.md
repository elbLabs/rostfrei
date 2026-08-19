# rostfrei-benchmarks

Rust-only workloads, paired comparisons and disposable NATS orchestration.
This workspace tool is not published as a framework library. Python is not
required for any benchmark command.

## Build once

Requires the repository Rust toolchain and Docker:

```sh
cargo build --locked --release -p rostfrei-benchmarks --bins
```

The coordinator runs the measurement binaries as separate processes, preserving
their runtime placement and keeping broker setup and comparison work outside
query timing. It uses a checksum-pinned NATS 2.12.1 container, loopback-only ports,
bounded readiness/subprocess waits and cleanup on success, failure or Ctrl-C/SIGTERM.
It ignores inherited `ROSTFREI_NATS_URL` unless an existing broker is explicitly
selected using `--nats-url`. No shell is used to execute benchmark binaries.

## Workloads

```sh
# Equivalent typed queries: KV, sequential/parallel replay, already-loaded replay.
target/release/rostfrei-benchmarks read-model -- \
  --events-per-aggregate 10,50,100 --samples 100 --rounds 3

# Full aggregate readiness, history, apply-only and an actual executor write.
target/release/rostfrei-benchmarks aggregate-loading -- \
  --events 0,100,1000 --samples 5 --output target/benchmarks/aggregate-loading.json

# Identical workloads/policies on preserved before/after executables.
target/release/rostfrei-benchmarks compare-history \
  --before /path/to/before --after target/release/read-model-benchmark \
  --baseline-revision BASELINE_SHA --output target/benchmarks/history-comparison.json
```

The comparison rotates execution order between five workloads: short direct
histories, long grouped histories, short and long trusted transaction histories,
and audited transactions. It rejects different policies, options, environments,
payload/result sizes, paths or sample counts. Reports retain both inputs, request
counts, latency summaries and executable SHA-256 hashes. Each completed workload
is saved atomically, preserving results if a later workload fails.

For a quick correctness check, not publishable performance measurements:

```sh
target/release/rostfrei-benchmarks compare-history \
  --before /path/to/before --after target/release/read-model-benchmark \
  --baseline-revision BASELINE_SHA --output target/benchmarks/smoke.json \
  --workload transactional,transactional-audited --events-per-aggregate 10 --quick
```

The measurement executables can also run directly with an explicit
`ROSTFREI_NATS_URL`. The coordinator supplies that variable only to child
processes; it never changes global environment state. To use an existing broker:

```sh
target/release/rostfrei-benchmarks --nats-url nats://127.0.0.1:4222 read-model -- \
  --events-per-aggregate 10 --samples 3 --rounds 1
```

## Tests

```sh
cargo test --locked -p rostfrei-benchmarks
cargo test --locked -p rostfrei-benchmarks --test runner -- \
  --include-ignored --test-threads=1

# Run any Rust integration command with the same isolated broker, no Python.
# Fresh checkouts must prefetch the macro test's deliberately offline fixture.
cargo fetch --locked --manifest-path crates/rostfrei-macros/tests/dependency-matrix/Cargo.toml
target/release/rostfrei-benchmarks run -- \
  cargo test --locked --workspace --all-features -- --test-threads=1
```

Broker-free tests cover fixtures, CLI/report contracts, paired comparison checks,
subprocess failures/timeouts and parity with every recorded comparison. Docker
lifecycle tests are opt-in. The separate 1,000-command historical audit experiment
is also opt-in and should be run in release mode, not with the default unit suite.

Methodology: [aggregate readiness](../../docs/benchmarks/aggregate-loading.md),
[read-model queries](../../docs/read-model-benchmark.md), and
[paired history replay](../../docs/history-replay-benchmark.md).
