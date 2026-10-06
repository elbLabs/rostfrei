# History-read optimization experiments — 26 September 2026

Related issue: [#79](https://github.com/elbLabs/rostfrei/issues/79).
Baseline: `44bba64f2c2ac756f2f06cd436fc6e62b79433ab`.

**Follow-up:** [round two](2026-09-26-history-read-windows.md) adds bounded
parallel leader-read windows, tighter ABBA measurements, and further validation.
The figures below describe the first optimization round.

## Result

Implemented an incremental optimization of authoritative transaction-history
validation. For a history of 100 separate one-event commands, the read count
dropped from **703 to 303 requests**. In a paired release run against the same
disposable NATS 2.12.1 broker, median history-load latency fell from **1,632 ms
to 345 ms** (five loads per implementation).

A separate Direct Get batch experiment retrieved and decoded 1,000 direct events
in **37–58 ms using eight batch requests**. It demonstrates a promising next
step, but is not used by the framework's authoritative loader: Direct Get can
read from lagging replicas, and adopting it requires a consistency design and
replicated-server tests.

The retained implementation preserves leader-routed reads, legacy receipt
precedence, event/receipt integrity checks, transaction read guards, and
attempt-scoped caches. There is no wire-format, public API, stream-policy, or
consumer-progress change.

## Iteration 1: reuse stream metadata

Receipt lookup and materialization previously fetched stream information three
times per historical transaction. Pass the existing stream handle through that
read instead. Fresh append-session incarnation checks remain in place; no
long-lived cached stream metadata is introduced.

- Measured 100-command history: **703 → 403 requests**.
- Event-store integration target: **44 passed** after this change.

## Iteration 2: reuse a checked first event

If the loaded commit starts at transaction event ordinal zero, it already
contains the global first event and its sequence/provenance information.
Reuse its stream identity instead of fetching that message again. When the first
event belongs to another aggregate, retain the leader lookup and verify its
operation identity and batch provenance.

The resulting receipt still has to cover the loaded commit exactly. Reusing a
decoded event does not mark its transaction as validated.

- 100-command history request budget: **303**.
- Added an integration regression for histories of 1 and 100 transactions,
  with a request budget that rejects per-transaction metadata and first-event
  rereads.
- Event-store integration target including the regression: **45 passed**.

## Iteration 3: pipeline independent receipt reads

Split receipt retrieval/decoding from participant materialization. Pipeline up
to **eight** independent, leader-routed receipt lookups through `FuturesOrdered`.
Materialize and validate the results in history order using the same raw-history
cache and snapshot cutoff as before.

Important properties:

- The pipeline has a bounded number of outstanding futures/results and creates
  no detached tasks.
- Errors/cancellation drop the pending futures with the enclosing load.
- Legacy-primary lookup still precedes operation-scoped lookup. A present but
  invalid legacy receipt does not silently fall through to another layout.
- Receipt, participant, checksum, identity, global-position, and guard validation
  still run before a history is returned.
- A repeated batch identity must cover every referencing commit exactly.
- No validated history is installed in an append session after a failed load.

This changes the amount of serialized waiting, not the remaining request count.
The count remains approximately `N + 2T + 3` for the specific current-layout,
single-writer, no-read-guard fixture. Multi-aggregate histories and legacy layouts
have different costs.

## Paired release measurements

The original release executable was preserved before modifying the framework.
Both executables ran sequentially against the same fresh NATS 2.12.1 broker and
independently seeded identical histories. Each removed its event store afterward.
Compilation, provisioning, and seeding were outside the timed `store.load()`.

| History | Events | Requests before → after | Median before → after |
| --- | ---: | ---: | ---: |
| One command transaction per event | 1 | 10 → 6 | 39.53 → 3.66 ms |
| One command transaction per event | 10 | 73 → 33 | 248.53 → 115.88 ms |
| One command transaction per event | 100 | 703 → 303 | 1,632.25 → 344.78 ms |
| Direct appends, control case | 100 | 102 → 102 | 533.85 → 544.25 ms |
| Direct appends, control case | 1,000 | 1,002 → 1,002 | 2,759.28 → 2,269.17 ms |

Raw times for the 100-command case, milliseconds:

```text
before: 1564.09, 3188.22, 1517.02, 2857.07, 1632.25
after:   379.74,  339.98,  214.54,  344.78,  487.22
```

This is about **57% fewer requests** and a **4.7× lower median** in this run.
The machine is shared, so timings remain diagnostic rather than production
latency guarantees. The unchanged direct-history request counts are useful
controls: this patch optimizes transaction validation, not initial event reads.
It also does not claim a 4.7× improvement to complete command execution, which
was not the timed operation.

## Separate batch-read experiment

An external probe explicitly enabled `allow_direct` on its own disposable stream
and issued subject-filtered Direct Get batches of at most 128 messages/4 MiB.
It captured the aggregate's last sequence through the leader-routed API before
reading. Each event was decoded through Rostfrei's observed-event codec, and the
result was compared with the existing authoritative loader's output.

| Direct history | Batch requests | Median | Range, five reads |
| --- | ---: | ---: | ---: |
| 100 events | 1 | 7.65 ms | 4.38–29.51 ms |
| 1,000 events | 8 | 38.62 ms | 36.58–58.26 ms |

The timings exclude the authoritative head lookup and the earlier load used to
obtain comparison data. The probe used direct, non-transactional history; it
does not demonstrate optimized receipt validation or replicated-cluster safety.

### Why this is an experiment rather than the production read path

[NATS ADR-31](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-31.md)
explicitly distinguishes the APIs:

- `$JS.API.STREAM.MSG.GET` routes to the stream leader and supplies
  read-after-write coherency.
- `$JS.API.DIRECT.GET` can be answered by a replica or participating mirror and
  does not itself guarantee read-after-write coherency.

Rostfrei's event-store policy currently does not enable Direct Get, and
[ADR 0005](../adr/0005-nats-event-store.md) keeps history reads independent from
consumer ACK progress. Automatically enabling a replica-read path or provisioning
read consumers would be a larger design change than these optimizations.

A promising next experiment is a **leader-anchored prefix read**: capture the
authoritative boundary, batch-fetch the immutable prefix, validate exact event
continuity and end identity plus transaction evidence, and fall back to the
leader path when a replica cannot provide that complete prefix. This must be
tested with lagging replicas, failover, recreation, truncated batches, and
concurrent appends before becoming a supported path. Provisioning and permission
requirements must remain explicit.

## Verification of the retained implementation

```sh
python3 scripts/test_nats.py -- cargo test --locked \
  -p rostfrei-nats -p bike-rental --all-features -- --test-threads=1

cargo clippy --locked -p rostfrei-nats --all-targets --all-features -- -D warnings
cargo fmt --all --check
git diff --check
```

The NATS/bike-rental run passed **242 tests**, with 10 ignored (five generated
domain metadata companions and five native-server authentication tests). Those
native authentication tests were previously run during the baseline review;
the optimization changes no authentication code. Clippy passed.

Coverage includes legacy-primary receipts/guards, forged receipts and transaction
coordinates, missing history, cross-aggregate transactions, read guards,
optimistic conflicts, exact replay, failed-load cache behavior, stream recreation,
directory snapshot races, durable domain-event delivery, and bike-rental's real
NATS transfer and Test-reset flows.

## Local experiment artifacts

- `/tmp/opencode/rostfrei-history-baseline`: preserved original release binary.
- `/tmp/opencode/rostfrei-review-probes`: release performance harness and
  `--batch-experiment` prototype.
- `/tmp/opencode/rostfrei-history-compare.py`: sequential baseline/optimized
  comparison runner.

Run the comparison from this checkout after building the optimized external
probe into `/tmp/opencode/model/target`:

```sh
python3 scripts/test_nats.py -- python3 /tmp/opencode/rostfrei-history-compare.py
```

These external artifacts contain checkout-local paths. The permanent regression
is `crates/rostfrei-nats/tests/event_store/history_reads.rs`.
