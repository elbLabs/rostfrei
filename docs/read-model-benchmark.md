# Read-model query benchmark: KV versus two-aggregate replay

## What is compared?

Two connected aggregate streams produce one organization-access query result:

```text
Organization                              BillingAccount
  Created { billing_account_id }            Opened { organization_id }
  MemberJoined { member_id } ...            SeatsPurchased { seats: 2 } ...
                   \                       /
                    OrganizationAccess view
                    - organization_id
                    - billing_account_id
                    - member_count
                    - purchased_seats
                    - available_seats
```

An event count includes the opening event in each stream. For 50 events per
aggregate, the result is 49 members, 98 purchased seats, and 49 available seats.
Both directions of the aggregate relationship are checked. The organization
rehydration builds a set of member identities; the read model stores their count.

The [benchmark](../crates/rostfrei-nats/examples/read_model_benchmark/main.rs)
measures four implementations of the same typed `QueryHandler`:

| Path | Work inside each timed query |
| --- | --- |
| `kv_snapshot` | Read one value through `NatsReadModelStore`, decode its versioned JSON envelope, return its view |
| `replay_sequential` | Load/validate organization history through `NatsEventStore`, decode/apply events, discover the billing ID, load/validate/decode/apply billing history, join the results |
| `replay_parallel` | Load both histories concurrently, decode/apply both, join; this gives replay the advantage of knowing the related ID in advance |
| `replay_loaded_history` | Decode/apply the same already-loaded `RecordedEvent` arrays and join; isolates in-process replay from broker I/O and event-store validation |

Every path also serializes the **same response JSON** inside the timer. Every
warmup and measured result must match byte-for-byte. Seeded member/seat counts and
source versions are independently checked before measurements. Errors abort the
benchmark instead of becoming successful latency samples.

The recorded KV case uses the underlying typed storage API with manually supplied
checkpoints. The declarative `ReadModel` runtime adds its own checkpoint envelope;
these measurements are the storage baseline, not a separate measurement of that
new runtime's reader and event-processing overhead.

## Reproduce

Requires the repository's Rust toolchain, Python 3.11+, and Docker. Build first,
then run against the checksum-pinned disposable NATS 2.12.1 fixture:

```sh
cargo build --locked --release -p rostfrei-nats --example read_model_benchmark

python3 scripts/test_nats.py -- cargo run --locked --release \
  -p rostfrei-nats --example read_model_benchmark -- \
  --events-per-aggregate 10,50,100 \
  --samples 100 --rounds 3 --warmup 10 \
  --events-per-commit 1 --extra-event-bytes 256
```

The counts above cover **20, 100, and 200 total events**. Use
`--events-per-aggregate 100` for two aggregates with 100 events each.
`--help` lists the parameters. `--events-per-commit` changes commit grouping; the
measured baseline uses one event per commit, representing one-event commands.
`--extra-event-bytes` adds padding beyond the event fields and JSON framing.

The executable emits a JSON report to stdout and progress to stderr. The Python
runner also prints its broker-startup line. Each invocation uses a unique Test
application namespace and deletes its own resources on completion/error; the
runner removes its container. Direct runs require `ROSTFREI_NATS_URL`.

For a fast correctness check, including a partially filled final commit:

```sh
python3 scripts/test_nats.py -- cargo run --locked --release \
  -p rostfrei-nats --example read_model_benchmark -- \
  --events-per-aggregate 2,3 --events-per-commit 2 \
  --samples 3 --rounds 1 --warmup 1
```

## Recorded results — 2026-10-02

Environment: Linux x86-64 KVM VM, 16 exposed AMD EPYC CPU cores, Rust 1.98.0
release build, two Tokio workers, NATS 2.12.1 in local Docker over a loopback
published port. Both stores use file storage and one replica. Histories and the
snapshot are warm; there are no concurrent source writers or competing benchmark
queries. Each path has 300 samples per case, after 10 warmups. Path order rotates
every iteration, interleaving the four implementations.

Full output, including means, p99, payload sizes, and observed client traffic:
[read-model-2026-10-02.json](benchmarks/read-model-2026-10-02.json).

**Latency in milliseconds: p50 / p95** (nearest-rank percentiles):

| Events per aggregate | Total events | KV snapshot | Replay sequential | Replay parallel | Replay loaded history, CPU only |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 10 | 20 | **0.849 / 1.960** | 23.387 / 34.575 | 13.360 / 21.344 | 0.021 / 0.031 |
| 50 | 100 | **1.229 / 3.070** | 118.079 / 173.239 | 66.995 / 98.565 | 0.086 / 0.140 |
| 100 | 200 | **1.279 / 3.281** | 231.836 / 413.025 | 134.290 / 220.973 | 0.183 / 0.264 |

For 100 total events, KV has **96× lower median latency than sequential replay**
and **54× lower than parallel replay** on this run. For 200 total events, the
ratios are approximately **181×** and **105×** respectively.

**Observed broker cost per query:**

| Total events | KV requests | Replay requests, either network path | KV bytes received | Replay bytes received |
| ---: | ---: | ---: | ---: | ---: |
| 20 | 1 | 24 | 640 | ~56,128 |
| 100 | 1 | 104 | 641 | ~251,689 |
| 200 | 1 | 204 | 651 | ~496,641 |

Counts come from the NATS client's published-message and received-byte counters
around each query. The client has no other application traffic during sampling.
Received bytes include stored event envelopes and API replies; they are not just
business payloads and are not full TCP/IP wire-byte accounting. For 200 events,
business event payloads total 63,805 bytes, the encoded KV snapshot is 212 bytes,
and the identical response JSON is 125 bytes.

**Serial service rate (`1 / mean latency`), queries/second:**

| Total events | KV | Sequential replay | Parallel replay | Loaded-history replay |
| ---: | ---: | ---: | ---: | ---: |
| 20 | 1,006 | 41.4 | 70.1 | 45,026 |
| 100 | 713 | 8.2 | 14.2 | 10,750 |
| 200 | 651 | 3.9 | 6.8 | 5,291 |

These are service-rate equivalents at concurrency one, **not measured saturation
throughput**. The benchmark interleaves paths, so the overall run's QPS is lower.
VM scheduling and broker latency vary; the KV cost stays one request even though
its measured latency changes between cases.

## Why the difference is large

The current `NatsEventStore::load_raw_history_through` performs a stream-info
request, a last-message lookup, and then one request per historical event.
For these independently committed source streams that yields:

```text
2 aggregates × (events per aggregate + 2 requests)
```

Parallel replay overlaps the two aggregate loads, but does not eliminate their
requests or payloads. KV reads one compact snapshot. Its query cost depends on
the snapshot's size, rather than the number of events used to construct it.

The CPU-only result is essential: replaying 100 already-loaded events takes
about **86 microseconds**, which is **faster than this remote KV read**. Most of
the network-replay penalty here is the current event-store access pattern,
serialization/validation, and repeated broker round trips. These results do not
mean that applying 100 events inherently takes 118 ms. A batched history reader,
aggregate snapshots, or an application cache would be different comparisons.

## Timing boundary and trade-offs

Included: typed handler execution, storage calls for the network paths, typed
JSON decoding, state reconstruction/join where applicable, and response JSON
serialization. All paths use the same open managed NATS connection.

Outside the timer: connection/provisioning, event seeding, snapshot construction
and creation, query-envelope construction/cloning, warmup, correctness comparisons,
and cleanup. The handlers are called directly; HTTP, a QueryBus request/reply
transport, authentication middleware, and source event delivery are outside this
measurement. The cached-history path starts after event-store validation.

The benchmark uses a single organization/billing pair per event-count case, warm
OS/broker caches, bounded small snapshots, no contention, and one replica. It
does not measure fleet-wide key distributions, cold disk, multi-node replication,
high query concurrency, projection write contention, or consumer lag. Three rounds
are pooled samples in one run, not three independent deployment trials.

In a running application, event handlers pay the incremental materialization
cost: read the current snapshot, apply a source event, CAS-write value and source
checkpoint, then ACK. The snapshot can lag its sources. Thus this moves work from
every query to event processing; it is not free elimination of work or a freshness
guarantee. Replaying two streams independently also requires an application
consistency contract if they change while being queried. This benchmark freezes
source writes and verifies identical results.

For query-heavy, key-addressed joined views, KV is a strong fit with this reader.
For the implementation, multiple-handler example, retry/ordering semantics, and
rebuild procedure, see [KV-backed read models](read-models.md).
