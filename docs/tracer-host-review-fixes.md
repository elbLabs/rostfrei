# PR #107 integration and lifecycle follow-up

## Continuous observation (#96 / triage 1)

Merged `origin/main` at `4bd956e92d89cc76466cf26cc697f49f1d8e0c13`
into the published feature branch without rewriting history. The two bike-rental
composition conflicts retain the shared typed host; continuous-observation feeds,
readiness callbacks and source guards move into that framework-owned lifecycle.

Behavioral red on disposable NATS 2.12.1:

```sh
python3 scripts/test_nats.py -- cargo test --locked -p bike-rental \
  --test nats_runtime_integration \
  integration_event_mapping_dispatches_a_command_through_nats -- --exact --nocapture
```

The combined branch compiled and failed with **observer was not ready before
external publication**. The new `shared_host_observation` acceptance also compiled
and failed with **Test ready before traffic: Unavailable != Live** before the
host wiring was added. An initial test compilation/setup error is not counted as
behavioral red.

Green: all three original `nats_runtime_integration` tests passed. The new host
test verifies Test/Production capability discovery, readiness before publication,
external events with persisted identities and explicit causation, default payload
redaction, Test reset isolation and source release on shutdown. Further focused
checks pass for broker stream interruption/recovery, failed reset/recovery,
observer startup rollback and invalid feed/runtime configuration.

Public API: `TracerConfig::with_continuous_observation(Arc<ObservationFeed>)`,
matching `TracerBuilder`'s replace-per-scope semantics. Bike-rental's `--tracer`
startup configures both feeds; Production Observe uses the inspection capability.
The standalone consumer and live host checker exercise this public configuration.

## Teardown admission (triage 2)

Behavioral red: `shared_host_admission` compiled and all three real-NATS tests
failed because **new submission waited for a response after teardown began**.
The explicit stop/host tests first block a genuinely admitted command in its
handler, begin teardown, and then attempt a new submission.

The fix checks admission before and after the submission gate, closes it at stop
initiation, and permanently closes both owned runtime scopes at application
shutdown/Drop/supervised failure. Raw runtime Drop closes retained senders too.
Tests check `Unavailable`, an unchanged durable command-stream sequence, and an
accepted outcome for the already admitted command. Host coverage includes explicit
shutdown, Drop, and `run_until` shutdown signals with Tracer and Normal attached.

Green: all three `shared_host_admission` tests pass against disposable NATS.

## Typed producer routing (triage 3)

Behavioral red: `producer_context_routing::typed_handler_subscribes_to_declared_producer`
compiled, printed a declared `...test.integration.producer.producer-changed`
address against the host's incorrect `...test.integration.bike-rental.producer-changed`
subscription, and timed out waiting for delivery on real NATS.

Green: both routing tests pass. A typed handler receives the declared producer
subject; the new `register_integration_command::<Event, _>(mapper)` path dispatches
and commits the mapped command in Test and Normal independently. Tests verify
consumer-owned distinct durables, same-context fallback, duplicate publication,
actual committed/response identities and correlation, and two producers sharing
an event name/schema with different observation codecs. Applications no longer
assemble the mapper's command bus, durable or subject.

## Complete split provisioning (triage 4)

Behavioral red on real NATS: `split_runtime_provisioning` called
`NatsRuntimeConfig::provision` twice then `connect_and_start`, and failed with
**failed to get durable consumer: consumer not found**.

The bindings-derived resource plan now includes the domain-event durable required
by integration mappings. Config provisioning, runtime convenience provisioning and
Test reset use that single plan; bike-rental's duplicate provisioning step is
removed. The regression checks independent/idempotent provisioning, a mapped
command's committed event and published integration event with real provenance,
and verification-only connect after deliberately deleting the required durable.

Green: the complete split-lifecycle test passes against disposable NATS.

## Combined verification (2026-10-13)

Commands used a process-local Cargo PATH and the repository disposable NATS 2.12.1
runner. Each broker and application namespace belonged to these checks.

| Check | Result |
| --- | --- |
| `python3 scripts/test_nats.py -- cargo test --locked -p rostfrei-tracer -p rostfrei-nats -p bike-rental --all-features -- --test-threads=1` | **469 reported passes, 0 failures, 12 ignored** across 40 Cargo summaries; includes all new regressions and current-main observation/reset tests |
| `python3 scripts/test_nats.py -- python3 scripts/check_tracer_host.py` | **5 standalone tests passed**, plus executable discovery/suite/shutdown; every Rostfrei dependency resolved from committed Git revision `03706adf67353f1181b0f2f4407f07346235af87` |
| `python3 scripts/test_nats.py -- python3 scripts/check_bike_rental_host.py --studio` | **5 real HTTP and 5 real Studio cases passed**, Test/Production Observe discovery and readiness passed, Normal state survived Test resets, clean supervised shutdown |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed; final feature-gating cleanup also passed targeted all-feature Tracer Clippy |
| `cargo check --locked -p rostfrei-tracer --no-default-features` | Passed without warnings after marking host-only metadata appropriately |
| `cargo rostfrei-dev check --workspace` | Passed |
| `cargo fmt --all --check`, `git diff --check` | Passed |
| `python3 -m unittest discover -s scripts -p 'test_*.py'` | **21 passed** |
| Studio `pnpm lint`, `pnpm build`, `pnpm test:ui` | Passed, including existing continuous-observation and credential/reset/SSE regressions |

The 12 ignored cases are five metadata companions, five opt-in native NATS
authentication/TLS cases, one subprocess entry point and one diagnostic benchmark.
They are not counted as passes. The external pinned qualification preceded only
lint/feature-gating cleanup, documentation and the live-checker's exact optional
favicon-404 allowance; the latter keeps all application/API HTTP errors fatal.

Browser checks used Node 24 and Chrome 146 with locally extracted shared libraries
and the existing `CHROME_NO_SANDBOX=1` runner option. Missing browser libraries and
sandbox support blocked initial launches; those attempts are not test passes.
The UI suite emitted its existing non-fatal ResizeObserver development warning.
