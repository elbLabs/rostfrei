# Aggregate history replay: paired before/after benchmark

## Change and measured boundary

Issue [#97](https://github.com/elbLabs/rostfrei/issues/97) replaces the initial
per-event raw-message loop with bounded, exact-subject ephemeral JetStream replay.
See [ADR 0042](adr/0042-batched-aggregate-history-replay.md) for read cutoffs,
validation, cleanup, permissions and retained operational trade-offs.
The implementation is rebased onto PR #89: trusted reads remain the default,
and strict handles/explicit historical audits keep their existing evidence checks.

The benchmark uses the same four typed query paths as the
[original read-model benchmark](read-model-benchmark.md): a KV read, sequential
replay of two related aggregates, parallel replay of those aggregates, and replay
of already-loaded history. Each path returns and serializes the same result;
every warmup and sample must match byte-for-byte. Network replay includes history
retrieval and integrity/transaction validation, not just event application.

The current before adapter is revision
`254869e8b26e58cbd8893a9f2d0812b354544486` (main including PR #89).
Both binaries use the same expanded benchmark harness: `--transactional` seeds
receipt-bearing command histories and `--history-auditing` enables historical
checks during measured reads. Both sides use the same read policy for every case.
The older pre-#89 comparison is retained below as historical evidence, not as a
claim of improvement over current main.

## Workloads

| Workload | Events per aggregate | Events per commit | Samples per path | Warmups |
| --- | --- | --- | --- | --- |
| Original single-event commits | 10, 50, 100 | 1 | 100 × 3 rounds | 10 |
| Long histories | 500, 1,000 | 99 | 20 × 2 rounds | 5 |
| Command-style transactions, trusted | 10, 50, 100 | 1 plus acceptance receipt | 30 × 2 rounds | 5 |
| Long command histories, trusted | 500, 1,000 | 1 plus acceptance receipt | 20 × 2 rounds | 5 |
| Command-style transactions, audited | 10, 50, 100 | 1 plus acceptance receipt | 30 × 2 rounds | 5 |

Each case has two aggregates. All cases add 256 padding bytes per event.
Long-history commits are grouped to keep fixture preparation tractable on the
old reader; they are a distinct workload, not an extrapolation of single-event
commands. Transactional cases use `append_transaction`; only audited cases
exercise historical receipt/provenance verification. They measure query
rehydration of command-style histories, not end-to-end command throughput.

Both release binaries run sequentially against the same disposable, pinned NATS
2.12.1 server. The harness alternates before/after order between workloads and
rotates the four query paths within each measured round. It verifies matching
options, versions, payload/result sizes and sample counts before comparing.

## Reproduce

The workloads and Rust-only runner now live in `rostfrei-benchmarks`.
Build the current read-model harness against the baseline adapter in
a separate checkout/target directory and save its release executable as `before`.
Only copy the benchmark harness into that baseline checkout; do not copy the new
adapter. Build the same harness against the new adapter as `after`:

```sh
git worktree add --detach ../rostfrei-history-baseline \
  254869e8b26e58cbd8893a9f2d0812b354544486
cp crates/rostfrei-benchmarks/src/bin/read-model-benchmark/{main.rs,model.rs,metrics.rs} \
  ../rostfrei-history-baseline/crates/rostfrei-nats/examples/read_model_benchmark/
cargo build --locked --release \
  --manifest-path ../rostfrei-history-baseline/Cargo.toml \
  --target-dir ../rostfrei-history-baseline/target \
  -p rostfrei-nats --example read_model_benchmark

cargo build --locked --release -p rostfrei-benchmarks --bins

target/release/rostfrei-benchmarks compare-history \
  --before ../rostfrei-history-baseline/target/release/examples/read_model_benchmark \
  --after target/release/read-model-benchmark \
  --baseline-revision 254869e8b26e58cbd8893a9f2d0812b354544486 \
  --output docs/benchmarks/history-replay-main-2026-10-07.json
```

The baseline must support both added harness flags. Build and preserve it before
building the new adapter. Seeding uses ordinary reads regardless of the measured
policy, avoiding quadratic historical audits during setup. Seeding, provisioning,
connection and cleanup are outside query timing, except for the new reader's
replay-consumer creation/deletion, which are deliberately included.

The baseline checkout retains its old Cargo example target solely to link the
identical harness against the old adapter. Current workloads, Docker lifecycle,
comparison logic and comparison tests all belong to `rostfrei-benchmarks`; the
Python benchmark scripts have been removed. The recorded JSON below is preserved
unchanged, and Rust tests reproduce every original comparison from those inputs.

## Results against main including #89 — 2026-10-07

Baseline: `254869e8b26e58cbd8893a9f2d0812b354544486`. This comparison measures
the additional batching benefit over #89's parallel raw windows and trusted-read
default, not the older serial/audit-every-load baseline. Both release binaries
were built with Cargo from byte-identical benchmark sources; only the event-store
implementation differs. The baseline checkout changed only the benchmark harness.

Environment: Rust 1.98.0, Linux x86-64 (`6.8.0-142-generic`, glibc 2.39), 16 exposed
logical CPUs, two Tokio workers, pinned NATS 2.12.1 in local Docker, file storage,
one replica. Tests and compilation finished before the measured run. Full reports,
run timestamp, binary SHA-256 hashes and all four query paths:
[history-replay-main-2026-10-07.json](benchmarks/history-replay-main-2026-10-07.json).

**Two-aggregate sequential replay, milliseconds p50 / p95:**

| Workload / policy | Total events | Main (#89) | Batched | Median ratio | Requests before → after |
| --- | ---: | ---: | ---: | ---: | ---: |
| Direct single-event commits | 20 | 19.820 / 30.049 | 18.024 / 28.959 | 1.10× | 24 → 10 |
| Direct single-event commits | 100 | 43.100 / 63.769 | 22.649 / 36.222 | 1.90× | 104 → 10 |
| Direct single-event commits | 200 | 68.866 / 107.472 | 29.830 / 49.762 | 2.31× | 204 → 10 |
| Direct, 99-event commits | 1,000 | 347.743 / 662.120 | 47.722 / 54.227 | 7.29× | 1,004 → 12 |
| Direct, 99-event commits | 2,000 | 618.245 / 1,152.875 | 73.405 / 98.591 | 8.42× | 2,004 → 16 |
| Transactions, trusted | 20 | 29.689 / 90.358 | 20.101 / 30.618 | 1.48× | 30 → 10 |
| Transactions, trusted | 100 | 63.466 / 97.374 | 28.374 / 52.087 | 2.24× | 110 → 10 |
| Transactions, trusted | 200 | 75.034 / 117.296 | 33.141 / 47.833 | 2.26× | 210 → 10 |
| Transactions, trusted | 1,000 | 343.937 / 487.409 | 62.478 / 80.484 | 5.50× | 1,010 → 12 |
| Transactions, trusted | 2,000 | 643.223 / 1,296.574 | 117.302 / 244.134 | 5.48× | 2,032 → 16 |
| Transactions, audited | 20 | 38.501 / 58.409 | 39.734 / 57.539 | 0.97× | 72 → 52 |
| Transactions, audited | 100 | 103.421 / 160.326 | 84.591 / 114.470 | 1.22× | 312 → 212 |
| Transactions, audited | 200 | 159.484 / 230.425 | 155.075 / 224.765 | 1.03× | 612 → 412 |

The command-style rows use one transaction and receipt per event, including the
1,000/2,000-event cases; they do not group 99 commands into one commit. These are
still query-replay measurements, not end-to-end `CommandExecutor` write timings.

Trusted command-history loads now benefit from both optimizations: #89 removes
deep evidence rereads from ordinary loads, while batching removes the per-event
retrieval requests. At 2,000 transactional events, received bytes fall from
5,072,669 to 3,746,671. The final query still transfers and applies every event.
For the measured workloads the page count is determined by the 256-message limit,
giving `2 × (4 + ceil(events_per_aggregate / 256))` requests, regardless of whether
events were published directly or transactionally. The old window reader can add
subject lookahead probes, hence slightly higher transactional request counts.

Strict loads still perform #89's receipt audits: at 200 events, 400 receipt
lookups and two audit metadata requests remain in addition to the 10 history
requests. Reducing 612 to 412 requests therefore does not remove the main audit
cost; the median gain is only 1.03×, and the 20-event audited case slightly regresses.
Audit checks and their eight-lookup pipeline are preserved, not disabled to
produce the trusted-read gain.

Unchanged KV control medians vary by 0.73–1.45×, and already-loaded replay also
varies, so the exact timing ratios are not production guarantees. All replay
paths check equivalent output; broker request counts provide the strongest
structural evidence. In this run both sequential and parallel trusted replay
improve across all measured history sizes, but fixed consumer setup/deletion
overhead means very short histories need not benefit on every deployment.

## Historical pre-#89 results — 2026-10-06

Baseline: `ef0f4e8079322268d263e5ae6a65c62631f1449a`, before the trusted-read
policy and parallel raw-reader changes. Transaction cases below audited history
on every load, as that baseline required. The expanded harness was compiled at
optimization level 3 against unchanged baseline release libraries before
rebuilding the adapter. These numbers are not comparisons against current main.

Environment: Linux x86-64 (`6.8.0-142-generic`, glibc 2.39), 16 exposed logical
CPUs, Rust 1.98 release optimization, two Tokio workers, pinned NATS 2.12.1 in
local Docker, file storage and one replica. No test or compilation jobs ran
alongside the measured comparison. Full reports, including all four paths,
p99/means and received bytes:
[history-replay-2026-10-06.json](benchmarks/history-replay-2026-10-06.json).

**Two-aggregate sequential replay, milliseconds p50 / p95:**

| Workload | Total events | Before | After | Median ratio | Requests before → after |
| --- | ---: | ---: | ---: | ---: | ---: |
| Single-event commits | 20 | 51.639 / 116.321 | 16.802 / 25.356 | 3.07× | 24 → 10 |
| Single-event commits | 100 | 196.145 / 303.549 | 26.254 / 51.751 | 7.47× | 104 → 10 |
| Single-event commits | 200 | 349.805 / 544.408 | 31.887 / 59.025 | 10.97× | 204 → 10 |
| Long histories, 99-event commits | 1,000 | 2,053.903 / 3,322.899 | 50.754 / 62.091 | 40.47× | 1,004 → 12 |
| Long histories, 99-event commits | 2,000 | 3,763.686 / 5,264.169 | 95.164 / 119.519 | 39.55× | 2,004 → 16 |
| One-event transactions | 20 | 254.483 / 394.075 | 151.816 / 412.540 | 1.68× | 146 → 132 |
| One-event transactions | 100 | 1,341.045 / 2,041.892 | 802.252 / 1,503.580 | 1.67× | 706 → 612 |
| One-event transactions | 200 | 2,182.965 / 3,365.146 | 1,942.159 / 2,696.661 | 1.12× | 1,406 → 1,212 |

Independent-history reads now use two stream/last-message lookups, one replay
creation, one or more pulls, and one deletion per aggregate. In these cases the
message-count limit determines the number of pulls, giving
`2 × (4 + ceil(events_per_aggregate / 256))` requests per query. Received bytes
also decrease: at 200 events, about 497 kB → 375 kB in decimal units
(496,651 → 375,232 bytes); at 2,000 events, 4,541,446 → 3,395,261 bytes. This removes
raw API response wrapping, not the need to transfer stored event envelopes.

### The historical command-history bottleneck

The old transaction workload shows why the original change was not a general 40×
command speedup. For these one-event, one-writer transactions, validation sent
six requests per historical transaction: a global-first-event lookup, legacy
receipt stream-info/lookup, current receipt stream-info/lookup, and stream-info
for receipt materialization. With 200 transactions, these contribute 1,200 of
the new path's 1,212 requests. Guards/other participants can add more work.

PR #89 subsequently removed historical audits from ordinary loads and reduced
redundant evidence lookups within audits. This rebased patch preserves that policy;
its trusted and audited results must be compared separately against #89's reader.

Timing variability is visible in the controls: transactional KV medians vary by
1.03–2.28× despite unchanged KV code, and the 20-event transactional parallel
case is slightly slower (139.438 → 144.901 ms). Some small transaction p95 values
also regress. Do not attribute all transaction timing differences to this patch;
the request-count reductions are the clearer structural result. Loaded-history
CPU timings remain broadly similar, confirming the improvement is retrieval,
not faster domain event application.

## Interpretation and limitations

These are warm-cache, loopback, one-replica, concurrency-one query measurements.
They are not saturation throughput, multi-node failover, production WAN latency,
cold-disk or projection-lag measurements. Pooled rounds run on one deployment;
they are not independent deployment trials. Before/after binaries do not share
the exact same seeded streams, but use identical workloads and result checks.

Client published-message counters measure request work, not TCP packets. Received
bytes include protocol/API overhead and stored envelopes, not just business
payloads. Consumer setup/deletion has fixed overhead and can be more expensive
than raw reads for very short histories. Full history still transfers/replays;
the API's returned `Vec` and append-session caches remain history-sized.

Strict/explicit transaction receipt/guard auditing remains raw-request-based.
Trusted command-history gains must not be presented as equivalent gains for deep
historical audits or end-to-end writes. KV queries still provide a separate
freshness/consistency trade-off rather than being interchangeable with replay.
