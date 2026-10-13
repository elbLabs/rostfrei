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
