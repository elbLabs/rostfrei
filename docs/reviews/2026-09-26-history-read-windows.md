# History-read optimization, round two: bounded leader-read windows

Related issue: [#79](https://github.com/elbLabs/rostfrei/issues/79).
Continues the [first experiments](2026-09-26-history-read-experiments.md).

**Subsequent measurement:** the [aggregate benchmark suite](../benchmarks/aggregate-loading.md)
now measures full readiness, apply, and executor writes. Its
[baseline results](../benchmarks/aggregate-loading-baseline.md) include a real
1,000-command history; the timings below remain history-only comparisons.

## Result

The authoritative loader now performs independent subject-filtered reads in
bounded windows and folds the results through the original ordered validators.
It uses the same **leader-routed raw-message API**. Direct Get, stream policy,
wire formats, and consumer progress are unaffected.

The final release comparison alternated serial and parallel strategies over the
same histories, connection, process, and runtime. Both strategies include the
previous receipt optimizations and the CPU changes described below:

| History | Serial median | Parallel median | Requests, serial → parallel |
| --- | ---: | ---: | ---: |
| 100 direct events | 234.52 ms | **84.84 ms** | 102 → 102 |
| 1,000 direct events | 2,051.73 ms | **530.86 ms** | 1,002 → 1,002 |
| 100 one-event command transactions | 418.63 ms | **235.35 ms** | 303 → 306 |

This run shows approximately **2.8×**, **3.9×**, and **1.8×** lower medians,
respectively. An earlier controlled window comparison also improved all three
cases, but with different timings. This shared host is noisy: these figures are
diagnostic measurements, not latency guarantees. The 100-command parallel samples
ranged from 79.99 to 291.96 ms; sub-100 ms is not yet a reliable result.

The request count remains proportional to history length. The window change
reduces serialized waiting; the previous receipt work reduced the actual number
of requests. Receipt selection and initial leader GET overhead remain important
targets for future work.

## Ten-agent proposal review

The user explicitly requested ten independent pitches. Each agent was assigned
read-only research; the implementation and benchmark execution remained in the
main session. Their main contributions and dispositions were:

| Focus | Proposal / finding | Disposition |
| --- | --- | --- |
| Leader-anchored Direct Get | Verify a complete immutable prefix against an authoritative boundary and retain fallback | Promising, deferred pending replicated-server and incarnation design |
| Server/API capabilities | NATS 2.12.1 raw GET explicitly rejects batch parameters; no supported leader-only batch endpoint | Retained raw GET; parallelize its independent requests |
| Receipt optimization | One positional receipt lookup could replace legacy/current probes, but changes off-batch shadow/precedence behavior | Deferred; keep legacy-first semantics |
| Attempt-local caching | Reuse participant raw prefixes, keeping pinned handler observations distinct from refreshable validation evidence | Deferred as a separate cache-lifetime change |
| Benchmark methodology | Fixed-order processes and root-future placement confound precise latency attribution | Added same-process, same-data ABBA comparison on a Tokio worker |
| CPU/allocation analysis | Commit lookup is quadratic over transaction histories; decoder copies a payload unnecessarily | Implemented binary search and copy removal |
| Adversarial consistency | Review boundaries, omissions, snapshots, cancellation, legacy selection, and stream recreation | Added targeted tests; final source review found no blocking defect |
| Batch transport | EOB, metadata provenance, byte overshoot, and subscription bounds need more work in the Direct Get prototype | Kept that protocol prototype out of production loading |
| Index/checkpoint design | Certified checkpoints and exact identity indexes can bound future prefix work | Longer-term wire/API design, not a shortcut in this patch |
| Minimal read optimization | Partition captured sequence ranges and keep one ordered fold | Implemented, then refined into small waves with sparse-gap skipping |

The server/API conclusions were checked against the pinned NATS 2.12.1 source,
not inferred from generated API documentation. See
[raw GET implementation](https://github.com/nats-io/nats-server/blob/v2.12.1/server/jetstream_api.go#L3531)
and [NATS ADR-31](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-31.md).

## Rejected first scheduling experiment

The first prototype split the entire global sequence span into eight ranges,
with one queued message per range. It passed integrity tests but did not provide
sustained concurrency: later ranges filled their tiny queues while the first
range continued making serial requests. Startup concurrency alone was misleading.

That implementation was replaced. A deterministic barrier test now requires all
eight windows to progress through an entire wave, rather than merely start their
first request.

## Retained window algorithm

Implementation: `crates/rostfrei-nats/src/event_store/history_reads.rs`.

1. Capture the existing authoritative upper boundary, including the existing
   optional snapshot cutoff.
2. Locate the first matching aggregate event through the leader API and retain
   it as a seed. This avoids parallel scans across unrelated history preceding
   a small/new aggregate.
3. Divide the remaining span into up to **eight windows**, each at most **128
   global sequence positions**. Adapt window width downward for short spans.
4. Fetch windows concurrently using the same exact-subject successor GET.
   Each chunk stops at a **256 KiB counted-byte target**, allowing one returned
   record to cross that target so supported historical messages remain readable.
5. Fold results in global sequence order through one `HistoryBuilder`. Commit,
   version, uniqueness, and predecessor state span all windows and chunks.
6. If a window stopped for its byte budget, finish its continuation before
   folding a later window.
7. Only the final completed window determines the next frontier. Its indexed
   successor lookup proves an empty gap; reuse the lookahead event as the next
   seed instead of rereading it. Discard unnecessary lookahead bodies immediately,
   including bodies beyond the snapshot cutoff.
8. If a nonempty wave contains fewer matching events than lanes, switch the
   remainder to one lane. This avoids multiplying every lookup by eight for
   extremely sparse histories.
9. Require complete terminal history and transaction validation before returning
   or installing anything in an append-session cache.

There are no spawned production tasks. `try_join_all` owns a wave's outstanding
futures; an error or cancellation drops those reads and accumulated results.
Producer-held/raw chunk storage is bounded, although total decoded history is
still proportional to event history. The byte target is not an RSS ceiling:
headers, container overhead, and an oversized historical record must also fit.

The fold treats aggregate versions as contiguous; global JetStream sequences may
legitimately have gaps. Range discovery never scans unrelated payloads to fill
those gaps. The new sparse regression spaces 100 events by a billion global
positions and limits the number of successor requests.

## Small CPU and ownership changes

In `event_store.rs`:

- Find a receipt participant's commit by **binary search** over the validated,
  ordered, nonempty commit list. Exact provenance checks still run after lookup.
- Decode directly into `RecordedEvent`, whose constructor already enforces the
  event envelope constraints; remove the disposable `NewEvent` payload clone.
- Move a uniquely owned standalone load's event vector using
  `Arc::unwrap_or_clone`, retaining the clone fallback when shared.

The benchmark compares serial/parallel transport with these changes present in
both. It does not separately quantify their CPU or allocation benefit.

## Controlled benchmark

An ignored diagnostic integration test provides a test-only serial strategy:

```sh
python3 scripts/test_nats.py -- cargo test --locked --release \
  -p rostfrei-nats --test event_store_integration \
  compare_serial_and_parallel_history_windows -- \
  --ignored --nocapture --test-threads=1
```

The comparison:

- Seeds each history once and warms both strategies.
- Uses one connection and a two-worker runtime, with the measurement on a worker.
- Executes three **ABBA blocks**, giving six measurements per strategy per case.
- Compares complete returned event vectors on every load.
- Measures `store.load()` only; seeding, compilation, and equality assertions
  are outside the timed interval.
- Records request counts, but asserts no fragile wall-clock threshold.

Final 100-command timings in milliseconds:

```text
serial:   337.102, 336.851, 500.157, 325.714, 555.008, 581.854
parallel: 291.959, 287.197,  98.706, 186.411, 284.286,  79.993
```

The full local output is
`/tmp/opencode/rostfrei-deep-review-window-benchmark-final.log`.
The earlier comparison is in `rostfrei-deep-review-window-benchmark.log`.

## Verification and review

- NATS and bike-rental suites: **265 passed, 11 ignored**. Ignored cases are
  five generated metadata companions, five native-auth fixtures, and the
  diagnostic benchmark. The benchmark was then run explicitly in release mode
  and passed.
- `cargo clippy --locked -p rostfrei-nats --all-targets --all-features -- -D warnings`
  passed.
- Formatting and diff whitespace checks passed.
- A separate adversarial source review found no blocking correctness issue in
  the final first-message capture, window/cursor transitions, lookahead ownership,
  sparse fallback, binary search, decoder validation, or owned-result extraction.

New coverage includes sustained concurrency, cancellation, later-read failure,
bounded chunks and oversized-message progress, wrong/regressing coordinates,
maximum sequence arithmetic, billion-position gaps, valid cross-wave commits,
omitted prefixes/interior commits/tails, byte-limit continuations followed by
carried lookahead, snapshot truncation, and malformed post-snapshot data.

Existing tests also continue to cover forged receipts and guards, legacy layouts,
multi-aggregate transactions, read guards, conflicts, exact replay, failed-load
cache behavior, stream recreation, and directory snapshot races.

## Remaining work

This preserves the current trust boundary and demonstrates further improvement,
but does not finish #79:

- The leader API still processes one request per event.
- Historical current-layout transactions still need the legacy miss plus current
  receipt lookup; changing that selection requires an explicit compatibility
  decision or a new storage-format discriminator.
- Large records can force serial chunk continuations; memory bounds take priority
  over maximum parallelism in that case.
- Very long histories still require history-sized decoding, storage, and replay.
- The reported measurements are history loads, not complete command throughput
  or production p95/p99 latency.

A genuinely batched authoritative prefix reader remains promising. It needs a
defined replica/incarnation contract and fault tests, rather than silently
enabling Direct Get or trusting a replica's end-of-batch response.
