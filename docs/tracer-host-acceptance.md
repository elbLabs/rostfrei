# Automatic NATS host acceptance (#69)

The first acceptance is `crates/rostfrei-tracer/tests/consumer`: a standalone
counter application with an injected handler dependency, a MessageSeries fixture,
and a JSON v2 behavior definition. It registers the command/handler once. The
check copies the consumer outside this workspace and exercises real HTTP and
JetStream, checking persisted history as well as the behavior report.

Run with:

```sh
python3 scripts/test_nats.py -- python3 scripts/check_tracer_host.py
```

The repository runner creates and removes its own NATS 2.12.1 container. It never
uses the default broker. A first local attempt failed before compilation because
`/tmp/opencode` was not writable; the system temporary-directory fallback is used
for the external qualification.

Red (2026-10-13): isolated NATS 2.12.1 ready; external consumer compilation
failed only with unresolved host/binding APIs (`E0432`, `E0433`, `E0425`). This
is a missing-API red, not an observed business-behavior failure.

Protocol check: the current catalog is **v1**, fixture documents are **v1**, and
behavior definitions are **v2**. The checked-in skill's aggregate-in-command
instructions predate bounded-context commands; code and advertised links govern.

## Acceptance coverage

| Contract | Executable evidence |
| --- | --- |
| Single registration, injected dependency, discovery, fixture, actual NATS execution | Standalone counter's `standalone_host_discovers_and_executes_behavior_through_real_nats`; runnable `src/main.rs` qualified separately |
| Repeated Given state, receipts reset, committed message/correlation/causation identities | Standalone repeated suite execution and persisted history assertions; bike-rental `nats_runtime_integration` |
| Preview is read-only; enabled HTTP/observer cleanup, including an open SSE response | Standalone HTTP test |
| Disabled command runtime and failed-reset recovery, preserving Normal state | `disabled_commands_work_and_failed_test_reset_fails_closed_without_touching_normal` |
| Active submissions exclude reset | `reset_waits_for_active_submissions`, with a deliberately blocked handler |
| Worker startup failure, unexpected exit, observer startup rollback | Standalone lifecycle tests with deleted broker resources |
| Fixture-derived integration suppression and existing production identities | Bike-rental runtime, transport and inspection integration regressions |
| All five registered behavior definitions through real HTTP and Studio | `check_bike_rental_host.py --studio` and `check_tracer_studio.mjs`, without browser request mocks |
| Production state survives Test suite resets | Live smoke rents in Normal scope, runs all Test definitions, verifies a second Normal rent rejects, then returns the bicycle |

The Git qualification copies the consumer outside the repository workspace and
pins every Rostfrei dependency to the committed checkout revision. It verifies
Cargo's resolved sources. This exercises the repository's Git distribution;
crates.io package verification is unavailable until the dependent Rostfrei
crates are published.

CI runs the workspace and external consumer against disposable NATS, and the
live HTTP/Studio suite in the Studio job. See [startup and API handoff](nats-application-host.md)
for the supported builder, the one-flag bike-rental startup, ownership policy,
measured code reduction, and the remaining #68/#101 boundaries.

## Deadline regression investigation

The existing inline payload-mismatch regression overrode the canonical suite's
35-second budget with 3 seconds. It intermittently cancelled the root command
before completion. Broker diagnostics showed a recreated durable delivering the
command, no quarantine entries, the two fixture events, and no exited workers.
In one captured failure, stream creation and fixture replay consumed about
2 seconds, leaving less than a second after command delivery. Separate traced
runs also exercised the existing response-reconciliation retry path (a 100 ms
read budget and 1-second retry delay).

The behavioral deadline starts **before reset**. The regression now retains the
canonical 35-second real-NATS budget and still requires an accepted root, actual
accepted response, failed comparison, and exact structured payload-mismatch
diagnostic. Transport retry/timeout policy and the behavior engine are unchanged.
Temporary broker/worker instrumentation was removed after investigation.

## Verification results (2026-10-13)

| Check | Actual outcome |
| --- | --- |
| `python3 scripts/test_nats.py` | Passed: 816 reported passes, 0 failures, 21 ignored across Cargo/subprocess summaries |
| Focused bike-rental behavioral integration regression after retaining the canonical budget | Passed: 1 test; accepted root and exact failed payload comparison verified |
| `python3 scripts/test_nats.py -- python3 scripts/check_tracer_host.py` | Passed: 5 standalone tests plus actual executable discovery, suite execution and clean shutdown; all Rostfrei dependencies resolved at `385c8fcb07caa4436676dc24badd10d4f8ab61de` |
| `python3 scripts/test_nats.py -- python3 scripts/check_bike_rental_host.py --studio` | Passed: 5 HTTP cases, 5 real-browser cases, production rent/rejection/return across Test resets, clean shutdown |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo fmt --all --check` and `git diff --check` | Passed |
| `cargo rostfrei-dev check --workspace` | Passed |
| `cargo check --locked -p rostfrei-tracer --no-default-features` | Passed |
| `python3 -m unittest discover -s scripts -p 'test_*.py'` | Passed: 21 tests |

The full workspace run included temporary diagnostic logging; the final focused
regression, Clippy and Git-installed external qualification ran after its removal.
The ignored tests comprise generated metadata companions, opt-in benchmarks and
Docker lifecycle tests, a subprocess entry point, and five native authentication/
TLS tests. They are not claimed as local passes. The live Studio run emitted a
ResizeObserver-loop development warning while its case, browser-error and HTTP-error
assertions passed.

No local #69 acceptance blocker remains. Remote PR CI is additional evidence;
#68 cross-process run identity, #101's full compatibility diagnostics, and
crates.io publication are outside this implementation's completion claim.
