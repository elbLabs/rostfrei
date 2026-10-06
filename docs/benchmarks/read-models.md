# Read-model query benchmarks

The benchmark compares an organization-access query over two linked aggregates:
organization membership and billing seats. Every path returns and serializes the
same JSON, checked byte-for-byte on every query.

| Path | Timed work |
| --- | --- |
| `kv_snapshot` | Read one typed KV value and decode its versioned envelope. |
| `replay_sequential` | Load/replay organization history, discover the billing ID, then load/replay billing history and join. |
| `replay_parallel` | Load both histories concurrently with both IDs known, replay, and join. |
| `replay_loaded_history` | Decode/apply already-loaded validated histories and join. |

Typed handler execution and response serialization are timed. Provisioning,
seeding, snapshot creation, warmup, correctness checks, and cleanup are outside
the timer, as are HTTP/QueryBus transport and projection maintenance. The KV path
measures typed storage, not the declarative runtime's checkpoint envelope.

## Run

With the repository toolchain and Docker:

```sh
cargo build --locked --release -p rostfrei-benchmarks --bins
target/release/rostfrei-benchmarks read-model -- \
  --events-per-aggregate 10,50,100 \
  --samples 100 --rounds 3 --warmup 10 \
  --events-per-commit 1 --extra-event-bytes 256
```

Counts include each aggregate's opening event, so this covers 20, 100, and 200
total events. `--events-per-commit` controls grouping; `--extra-event-bytes` adds
payload padding. Use `--help` for other options. The runner starts disposable NATS
2.12.1, uses a unique Test namespace, and cleans up. Direct binary runs require
`ROSTFREI_NATS_URL`.

JSON goes to stdout and progress to stderr. Reports include raw timing summaries,
payload sizes, and client traffic counters. Request counts are client publications,
not TCP packets; received bytes include envelopes and replies.

For a quick correctness check, including a partial final commit:

```sh
target/release/rostfrei-benchmarks read-model -- \
  --events-per-aggregate 2,3 --events-per-commit 2 \
  --samples 3 --rounds 1 --warmup 1
```

Use the [paired comparison runner](../../crates/rostfrei-benchmarks/README.md#workloads)
for before/after binaries with matching workload and policy. Retain revision,
environment, and raw results alongside performance reports.

## Interpretation

The workload has warm history, one organization/billing pair per case, no concurrent
writers, and one replica. It does not measure saturation, consumer lag, cold disk,
or production contention. Loaded-history replay separates CPU work from retrieval;
KV shifts materialization cost into event processing and can lag its sources.
Replaying changing streams also needs an application consistency contract.

The historical paired reports in this directory are retained as comparison-format
regression fixtures. Rerun current binaries for performance claims. See
[read models](../read-models.md) for runtime tradeoffs.
