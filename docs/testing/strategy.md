# Testing Rostfrei itself

## Policy

**Use a compact set of real-NATS integration contracts as the backbone of runtime
testing, supported by focused broker-free tests and a few complete application
journeys.** Optimize for confidence, understandable failures, and fast feedback.

This is the development policy for the framework repository.

### Give each contract an owner

Before adding a test, answer:

1. What observable behavior or promise could break?
2. Which existing suite owns it?
3. What does this test prove that the existing assertions do not?
4. Does that proof require the real broker, a public adapter, or only a function?

Usually one representative successful scenario and the meaningful failure
classes are sufficient. Add a further case when it exercises a different state
transition, persistence boundary, trust boundary, or a reproduced bug. There is
no requirement to enumerate every input variation or every combination of
adapter, decision, schema, and fault.

Do not add tests for straightforward getters, derived traits, private call order,
or a dependency's documented behavior unless Rostfrei adds a relevant contract.
Treat tests that only reproduce the implementation as removal candidates.

Keep the suite deliberately small. When an integration contract already catches
the same failure, remove redundant lower-level scenarios unless they provide
independent algorithm, boundary, or diagnostic value. Start with a representative
success and the materially different failure classes. Extra variations need a
specific reason to exist.

Moving every old case into a table or a faster layer preserves the maintenance
burden. Actually remove equivalent input variants, repeated adapter permutations,
and superseded reproductions. Retain supported compatibility contracts and
distinct high-impact regressions at their owning layer.

## Where behavior belongs

| Layer | Owns | Typical proof |
| --- | --- | --- |
| Broker-free contracts | Identity/codec rules, retry and authorization policy, graph comparison, macro diagnostics, typed API behavior | A small table of semantic equivalence classes; compile-pass/fail for public macro guarantees |
| Real NATS | JetStream/KV persistence, command delivery, atomic commits, durable replay, consumer progress, scope isolation, provisioning | Public adapter/worker + real broker + independently read persisted result |
| Focused resilience | Uncertain acknowledgements, restart, authority recreation, expiry races, sink outage | A controlled fault at a named boundary, proof the fault occurred, and an independent recovery observation |
| Application acceptance | Framework composition, startup/configuration, HTTP/CLI/Studio wiring, external dependency/package usability | A small number of complete journeys using the actual application-linked host |

An in-memory store still needs its shared backend conformance suite: it is a
supported implementation. Reuse the contract definitions in `rostfrei-testing`
for memory and NATS. Do not independently re-author their business scenarios.

HTTP status mappings belong to `rostfrei-http`; durable outcome semantics belong
to core/NATS. Test the HTTP mapping locally and retain a representative real
HTTP-to-NATS journey. The complete outcome matrix need not run through both.

### Failure cases worth keeping

The following are central runtime promises, even if failures are uncommon:

- An accepted, accepted-without-events, or rejected command has one durable
  outcome. Exact retry cannot reevaluate it or produce a second effect.
- A conflicting multi-aggregate transaction leaves no partial effects. Read
  guards also apply to rejections and operations that emit no events.
- Lost acknowledgements and process/broker restart cannot turn a committed
  decision into executable absence. Preparation alone is not a decision.
- Missing or recreated authority, unavailable details, and expired details remain
  distinguishable. A cached response cannot bypass current authorization or expiry.
- Application, context, traffic scope, operation identity, and actor attribution
  survive the boundaries that carry them. An actor claim is not authentication.
- Source ACK/TERM follows durable response or quarantine publication. Transient
  failures recover; poison messages do not indefinitely block healthy work.
- Concurrent workers respect the promised ordering/capacity rules. Read-model
  state and checkpoints do not double-apply or become partially persisted.

Preserve one decisive regression for each *distinct mechanism*. For example,
losing the preparation ACK, the final commit ACK, and the expiry ACK are different
state transitions. A test for one does not prove the others. Conversely, changing
three ordinary malformed field values rarely justifies three broker scenarios.

## Writing useful tests

- Describe setup, action, and observable result. Use behavior names rather than
  review rounds, implementation phases, or issue numbers; put bug history in a
  short comment when useful.
- Prefer public APIs. Use raw broker access when proving wire compatibility,
  stored evidence, or corruption handling; make that purpose explicit.
- Import the compiled crate from integration tests. Importing production `.rs`
  modules via `#[path]` or `include!` also imports their unit tests and recompiles
  the implementation. Keep private codec/invariant tests colocated in the
  library and share test-only helpers instead.
- For replay, check original result/evidence, persisted event count, and handler
  invocation count where relevant. A response status alone is weak evidence.
- Make failures diagnostic: name table rows and the boundary being faulted.
  A shared helper must not hide which scenario failed.
- Keep table-driven input cases together when they share one contract. Keep
  unrelated behaviors and independently meaningful failure points as separate
  tests, even when they share fixture setup.
- Use notifications, barriers, or the existing wire gate for synchronization.
  Poll a concrete state with a deadline instead of sleeping and assuming success.
- Shorten configurable test timeouts. Verify the production default separately
  from whether timeouts preserve durable work.
- A simulated error proves the caller's reaction to that error. A lost-PubAck
  claim requires an actual intercepted acknowledgement and a fault counter.
- Performance tests should express a deliberate resource bound. Exact private
  request sequences need a specific compatibility reason, rather than merely
  matching today's implementation.

### Fixtures

Use an owned disposable broker with random loopback ports and unique namespaces.
Keep one fixture per independently runnable scenario; share fixture construction,
not mutable scenario state. A restart test needs retained private storage and a
fresh client/process where that is the property under test.

Extend the existing NATS support modules before creating another broker launcher
or wire proxy. Share small capabilities such as a named request gate or ACK drop;
avoid a second implementation of the execution protocol inside the fixture.

Keep required tests enabled. Do not silently pass when NATS is missing or use
`#[ignore]` as a substitute for deciding when a required contract runs. Existing
ignored metadata companions and subprocess entry points are harness plumbing,
not missing runtime coverage.

## Development feedback loop

For a behavior change, first express the changed contract in its owning suite.
Establish a meaningful failing test, implement the behavior, then run the affected
boundary. A refactor covered by existing contracts may use those tests directly.
Documentation and low-impact mechanical changes need relevant validation rather
than newly invented tests.

Choose checks by the dependency impact of the change:

| Changed area | Local verification |
| --- | --- |
| Core identity, codec, executor, shared outcome contract | Owning broker-free tests, then affected real-NATS contracts; broaden for shared runtime changes |
| NATS storage, delivery, consumer or read model | Owning real-NATS target plus the relevant recovery/ordering scenario |
| HTTP or Tracer boundary | Local API contracts plus a live-host journey when wiring or execution changes |
| Macro or domain model | Compiled public behavior and affected trybuild cases; external dependency fixture when imports/features change |
| CLI, scaffolding, packaging | Owning CLI tests plus the external consumer/package check affected by the change |
| Studio | `studio/AGENTS.md`: lint/build, relevant interactions and visual inspection; live-host journey for backend wiring |
| Test runner or common fixtures | Runner checks and representative broker lifecycle/fault tests; full workspace for shared fixture changes |
| Documentation only | Review paths, commands, and consistency with current code |

Run formatting and affected Clippy checks for Rust changes. Before merging
cross-cutting runtime or consolidation changes, run the complete workspace gate
and the affected external checks. A passing focused test is useful feedback, not
evidence that an unrun gate passed.

## Commands available today

Run from the repository root with its pinned Rust toolchain. The standard NATS
runner needs Python 3.11+ and Docker. **Its default workspace command and
`--contracts` also require a cached native NATS 2.12.15 binary** for private
restart/fault fixtures, supplied on PATH or through `ROSTFREI_NATS_SERVER`.

```sh
# Full Rust workspace gate: authenticated broker plus restart fixture setup.
ROSTFREI_NATS_SERVER=/path/to/nats-server python3 scripts/test_nats.py --contracts

# Ordinary broker-backed target.
python3 scripts/test_nats.py -- cargo test --locked -p rostfrei-nats --test messaging_integration -- --test-threads=1

# Fault contracts that require authenticated/native fixture variables.
ROSTFREI_NATS_SERVER=/path/to/nats-server python3 scripts/test_nats.py --contracts -- cargo test --locked -p rostfrei-nats --test completed_outcome_faults -- --test-threads=1

# Broker-free owning suites.
cargo test --locked -p rostfrei-core
cargo test --locked -p rostfrei-tracer --features http
python3 -m unittest discover -s scripts -p 'test_*.py'

# Native authentication/TLS lifecycle; also requires OpenSSL.
ROSTFREI_NATS_SERVER=/path/to/nats-server cargo test --locked -p rostfrei-nats --test connection_integration -- --include-ignored --test-threads=1

# Working-tree application-linked host, then the standard bike-rental host.
python3 scripts/test_nats.py -- python3 scripts/check_tracer_host.py --working-tree
python3 scripts/test_nats.py -- python3 scripts/check_bike_rental_host.py
```

Without `--working-tree`, `check_tracer_host.py` qualifies the committed HEAD
through Git dependencies. Use that mode for committed-source acceptance; it does
not establish that uncommitted framework edits work.

The default runner prefetches the separate macro dependency-matrix lockfile.
When selecting that target directly, prepare it with
`cargo fetch --locked --manifest-path crates/rostfrei-macros/tests/dependency-matrix/Cargo.toml`.
The test deliberately builds offline.

For a complete gate, also use `cargo fmt --all --check`,
`cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`,
and `cargo rostfrei-dev check --workspace`, plus the area-specific external and
frontend checks in [the development guide](../development.md).

Report the changed contract, commands actually run, results, and any missing
prerequisite. Do not repeat passing checks without a new reason.
