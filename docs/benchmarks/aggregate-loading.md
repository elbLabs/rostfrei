# Aggregate readiness benchmarks

This suite measures the boundary an application cares about: **the aggregate is
rehydrated and ready to use**, including history loading, runtime validation,
typed event decoding, initialization, and event application.

It also measures history-only loading, apply-only work, and an actual
`CommandExecutor` write. These are distinct phases; history-read latency alone
does not establish aggregate-readiness or command latency.

The [initial measured baseline](aggregate-loading-baseline.md) includes complete
1,000-event rehydration for both direct and command-generated histories.

## Run

With the repository toolchain, Python 3.11+, and Docker:

```sh
python3 scripts/test_nats.py -- cargo bench --locked \
  -p rostfrei-nats --bench aggregate_loading -- \
  --events 0,100,1000 --samples 5 \
  --output /tmp/rostfrei-aggregate-loading.json
```

This starts a fresh pinned NATS 2.12.1 broker with file storage and one replica,
then cleans it up. The benchmark provisions a uniquely named Test-scoped event
store and removes it on success or a returned error. A direct invocation requires
an explicit, nonempty `ROSTFREI_NATS_URL`; the disposable runner is recommended.

The full matrix includes both history kinds. Creating 1,000 command-generated
events executes **1,000 real commands**, including their history loads and atomic
appends, so fixture creation can take considerably longer than the measured
samples. Setup time is reported separately and excluded from sample timings.

For a quick check or a larger payload:

```sh
python3 scripts/test_nats.py -- cargo bench --locked \
  -p rostfrei-nats --bench aggregate_loading -- \
  --events 0,10 --samples 3 --output /tmp/rostfrei-quick.json

python3 scripts/test_nats.py -- cargo bench --locked \
  -p rostfrei-nats --bench aggregate_loading -- \
  --history commands --events 1000 --note-bytes 1024 --samples 20 \
  --output /tmp/rostfrei-1k-command-history.json
```

`cargo bench` builds an optimized executable. Actual broker measurements reject
debug builds. The default artifact is the checkout's
`target/benchmarks/aggregate-loading.json`; `--output` can select another path.
Cargo's working-directory behavior does not affect the default artifact location.

## Workload

The benchmark uses a small inventory aggregate with 64 SKU identities and a
`BTreeMap` stock projection. Each JSON event contains a SKU, quantity, and note.
Applying an event updates stock and records event/note-byte totals. This exercises
typed JSON decoding, state initialization, allocation, and repeated entity-key
updates rather than treating any payload as an increment without decoding it.

The default note is 128 bytes. **That is not the total event size:** JSON fields,
storage envelopes, headers, receipts, and encoding add bytes. The report records
the exact seeded domain-payload total.

Two history kinds are supported:

| Kind | Fixture creation |
| --- | --- |
| `direct` | Low-level atomic event batches, bounded by both event count and payload bytes; no per-command receipts |
| `commands` | One typed handler execution and one event-producing transaction per event, with durable receipts |

The command-history case is the more representative baseline for ordinary
command-driven applications. A complex application's apply logic or growing
aggregate state may cost substantially more than this bounded inventory model.

## What each phase includes

Read phases use ordinary trusted loads by default. Add `--audit-reads` to include
deep historical transaction auditing in `history-load` and `rehydrate`, reproducing
the previous read policy. This flag does not change fixture writes or timed
executor-command semantics. The original baseline report predates this separation.
See the [trusted-versus-audited comparison](trusted-reads-experiment.md) for a
controlled A/B measurement on the same 1,000-command history.

| Phase | Timed boundary |
| --- | --- |
| `history-load` | `NatsEventStore::load()` through its returned locally checked event vector |
| **`rehydrate`** | **`CommandExecutor::rehydrate::<InventoryAggregate>()` through the returned ready-to-use aggregate** |
| `apply-only` | Aggregate initialization and applying an already decoded owned event vector |
| `execute-command` | A new `CommandExecutor::execute()` call, including loading, validation, typed decoding, apply, decision, encoding, concurrency checks, atomic append, and verification |

`rehydrate` additionally records two intervals **inside that same sample**:

- `history_load_ns`: time in the underlying event-history provider.
- `after_history_ns`: remaining runtime validation, typed decoding,
  initialization, and apply work before the aggregate is returned.

These paired intervals avoid estimating apply cost by subtracting the medians of
two independent, noisy benchmark phases. The second interval is broader than
`apply-only`, and includes scheduling/preemption time; it is not a CPU-cycle
measurement. Component medians need not sum to the median total.

For `apply-only`, preparing/cloning the typed input vector is outside the timer.
The timed initialization/application consumes that vector normally. This isolates
application of already decoded events; it does not replace the full-readiness
measurement.

The write phase measures the in-process executor. HTTP routing, command-bus
publication, worker queueing, durable command responses, and post-commit
integrations are outside this boundary.

## Controls and correctness

- Cases use a fresh application/event-store namespace. No snapshots or
  process-wide aggregate cache shortcut rehydration.
- The benchmark runs on a Tokio worker with an explicit worker count (default 2).
- Read phases rotate their execution order between samples and share the same
  immutable fixture. Explicit warmup count defaults to 1.
- Broker/file-system caches are not forcibly cold; even with zero explicit
  warmups, fixture creation may have warmed them.
- Connection flushes, input preparation, output checking, artifact writing,
  provisioning, and fixture generation are outside the sample timer.
- Every aggregate result is checked for event count, all per-SKU quantities, and
  note-byte totals. History reads check their length. Every timed command must
  append one **new** event, not return an idempotent replay. Final persisted state
  is rehydrated and verified.
- Write warmups target a separate aggregate. Timed command samples start at the
  advertised fixture size, then increase the same history by one per sample.
  `events_before` records the actual size; the summary includes its min/max.
- Timers stop when the result is ready, before result verification and disposal.
- NATS outbound message deltas come from the dedicated client connection.
  They count application requests/publications, not TCP packets or round trips.

## Report

The JSON artifact is written atomically after a successful run and cleanup.
Schema version 1 includes:

- Timestamp, Git revision/dirty flag, framework and Rust versions, server version,
  architecture, available CPU count, and configured runtime/workload parameters.
- Case setup time and exact seeded domain-payload bytes.
- **Every raw sample**, with phase, iteration, history size, duration in
  nanoseconds, request/publication count, and optional paired rehydration intervals.
- Sample count, minimum, median, maximum, request counts, and history-size ranges.
- A nearest-rank `p95_ns` only when at least 20 samples are available. Small runs
  intentionally do not report a misleading percentile.

Example extraction:

```python
import json

with open("/tmp/rostfrei-aggregate-loading.json") as source:
    report = json.load(source)

for case in report["cases"]:
    ready = next(s for s in case["summaries"] if s["phase"] == "rehydrate")
    print(case["history"], case["seeded_events"],
          f"ready median: {ready['median_ns'] / 1_000_000:.3f} ms",
          f"after history: {ready['median_after_history_ns'] / 1_000_000:.3f} ms")
```

Keep raw reports from both revisions, use the same payload/history/runtime
settings, avoid concurrent builds, and compare repeated runs on a controlled
machine. A five-sample median from a shared host is a diagnostic, not a production
latency objective. Even a 20-sample percentile is an observed sample statistic,
not a reliable production p95/p99 under load.

## Options

| Option | Default | Purpose |
| --- | --- | --- |
| `--events` | `0,100,1000` | Unique history sizes; zero covers initialization without events |
| `--history` | `direct,commands` | Select fixture provenance |
| `--samples` | `5` | Timed samples per phase |
| `--warmups` | `1` | Untimed warmups per read phase; writes warm a separate aggregate |
| `--audit-reads` | off | Audit historical receipts/participants during read phases |
| `--note-bytes` | `128` | JSON note length, up to 65,536 bytes |
| `--workers` | `2` | Explicit Tokio worker count |
| `--case-timeout-seconds` | `1800` | Whole-case bound, including fixture generation |
| `--stream-bytes` | `268435456` | Finite event-store byte capacity; increase for large matrices |
| `--output` | checkout `target/benchmarks/aggregate-loading.json` | JSON artifact path |

There are no pass/fail millisecond thresholds. Correctness, bounded configuration,
successful execution, and report generation determine success. Performance
thresholds should be added only for a controlled benchmark environment.

## Tests

The fixture and reporting contracts run without a broker:

```sh
cargo test --locked -p rostfrei-nats --test aggregate_benchmark
```

They cover direct/command fixture equivalence, usable reconstructed state,
idempotent replay, large-event batch boundaries, CLI validation, sample counts,
history growth, and quantile definitions. The actual NATS suite is exercised by
the benchmark command above. A normal debug-profile Cargo test invocation of the custom benchmark
runs the in-memory fixture smoke checks rather than publishing misleading debug
timings.
