# Trusted reads versus historical auditing

This reversible experiment implements
[ADR 0039](../adr/0039-trusted-event-store-reads-and-explicit-audits.md).
The earlier optimization and benchmark baseline was checkpointed as
`d2c9a17` before changing the read policy.

## Measured result

Both strategies rehydrated the **same aggregate with 1,000 actual command
transactions**, using the benchmark's JSON inventory model and 128-byte notes.
The timer ended when a usable `AggregateInstance` was returned.

| Read policy | Median | Range, six samples | Outbound requests |
| --- | ---: | ---: | ---: |
| Historical audit | **2,330.91 ms** | 1,418.32–2,926.22 ms | 3,017 |
| Trusted ordinary load | **603.62 ms** | 448.38–1,140.67 ms | 1,016 |

The experiment removed **2,001 requests** and produced a **3.86× lower median**
in this run. All returned states were identical and checked against the fixture.
The shared machine remains noisy; this is evidence of improvement, not a
production latency guarantee. The remaining roughly 1,000 event reads are still
a significant cost.

Raw full-rehydration times, milliseconds:

```text
audited: 2926.217, 1971.999, 2339.021, 2589.217, 1418.323, 2322.794
trusted:  947.109, 1140.668,  448.376,  576.873,  506.872,  630.371
```

## Method

- Optimized build, Rust 1.98.0, disposable NATS 2.12.1, file storage, one replica.
- One process/connection and a two-worker Tokio runtime; measurement on a worker.
- Seed 1,000 real commands once, then warm both read paths.
- Three ABBA blocks: audited, trusted, trusted, audited.
- Audit mode calls `audit_history` through an `EventHistory` wrapper and the
  same `CommandExecutor::rehydrate` implementation as ordinary mode.
- Setup, result verification, and cleanup are outside the timed interval.

Run the controlled comparison:

```sh
python3 scripts/test_nats.py -- cargo test --locked --release \
  -p rostfrei-nats --test history_audit_benchmark -- \
  --ignored --nocapture --test-threads=1
```

The general `aggregate_loading` benchmark also supports `--audit-reads` for
history-only and full-rehydration phases. Its writes remain ordinary, so that
flag should not be interpreted as recreating the old command-write path.

## What changed

Ordinary aggregate/session loads and directory discovery now trust writes made
through the supported event-store API. They retain local envelopes, checksums,
identities, versions, predecessor expectations, and complete local commit checks.
Locally provable atomic positions are also checked without receipt queries.

Historical receipt/participant/guard verification is explicit through
`audit_history` and `audit_streams`. Those methods preserve legacy receipt
selection and snapshot-aware participant validation.

Write admission, expected versions, read guards, atomic publication, verification
of newly appended data, exact replay and uncertain-write reconciliation remain
enforced. The relevant operation's receipt is still read when required. Durable
domain-event consumers retain their operation-specific transaction verification.

**Deliberate semantic change:** an out-of-band record with valid local structure
but missing/inconsistent historical receipt evidence can be returned by ordinary
reads. Auditing rejects it. Imports and administrative recovery can perform the
explicit audit before making that data available to normal execution.

## Verification

- Complete Rust workspace with disposable NATS: **734 passed, 0 failed, 18 ignored**
  (harness totals, including duplicated source modules across targets).
- The ignored A/B benchmark was separately executed in release mode and passed.
- An actual-broker `--audit-reads` smoke matrix passed for empty and three-command
  histories, including all four benchmark phases and persisted-state checks.
- Workspace Clippy, formatting, version consistency, and structure checks passed.
- Tests explicitly cover trusted versus audited missing-receipt behavior, local
  malformed-history rejection before writes, local batch coordinates, failed-load
  cache behavior, legacy guards, snapshot cutoffs, concurrency, exact replay,
  uncertain-publish reconciliation, and durable domain-event handling.

The changed trust-boundary tests replace historical-receipt assumptions with
explicit audit assertions; local-corruption/no-publication checks remain tested.

Local logs:

- `/tmp/opencode/rostfrei-deep-review-trusted-workspace.log`
- `/tmp/opencode/rostfrei-deep-review-trusted-audit-ab.log`
- `/tmp/opencode/rostfrei-audit-mode-smoke.json`

## Reversal

Revert the separate commit introducing ADR 0039 to restore historical audits
during normal reads. Earlier optimizations and the benchmark suite remain in the
baseline checkpoint. Stored data and wire formats require no rollback migration.
