# Application-linked NATS host

Enable `rostfrei-tracer`'s `nats-host` feature. `ApplicationBindings` is the
executable composition: register aggregate fixture codecs, command instances and
rejection mappers once. Those same registrations supply catalog metadata,
payload decoding and `CommandProcessor` execution. Dependencies stay in your
handlers. Integration mappings and handlers remain application-owned; the host
supplies their buses, durable consumers and fixture-event suppression.

The runnable minimal consumer is
[`tests/consumer/src/main.rs`](../crates/rostfrei-tracer/tests/consumer/src/main.rs),
with its composition in `tests/consumer/src/lib.rs`. Its entry point is ordinary
`cargo run`; no new Cargo subcommand or generated Tracer implementation is needed.

```rust,ignore
let bindings = Arc::new(application_bindings()?);
let tracer = TracerConfig::new(listener, HttpConfig::new(control_token)?)
    .with_test_repository(Arc::new(FilesystemTestRepository::load("tests/tracer")?))
    .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
let host = NatsApplication::builder(test_context, connection, bindings)?
    .with_fresh_completed_outcome_epoch("owned-disposable-test")?
    .with_default_test_fixture(fixture)
    .with_tracer(Some(tracer))
    .provision_and_start().await?;
host.run_until(shutdown_signal).await?;
```

`TracerConfig` accepts an already-bound TCP listener and existing HTTP capability
configuration. Payloads are redacted unless a policy is explicitly supplied.
Catalog v1 advertises fixture v1 and behavioral-definition v2 discovery links.
Repository validation, execution and evidence use the existing Tracer engines.

### Continuous external-flow observation

Use the same feed API as `TracerBuilder` on the host's `TracerConfig`:

```rust,ignore
let tracer = TracerConfig::new(listener, http)
    .with_dispatch_runtime(normal_runtime)
    .with_continuous_observation(ObservationFeed::new(
        application.clone(), ObservationScope::Test,
    ))
    .with_continuous_observation(ObservationFeed::new(
        application, ObservationScope::Production,
    ));
```

Feeds are opt-in; configuring a scope again replaces its previous feed. The host
checks each feed's application against its runtime. Production observation requires
the explicitly attached Normal runtime. HTTP discovery and access retain the
existing control (Test) and read-only inspection (Production) capabilities.

The framework registers a source named for each runtime's bounded context,
establishes both broker subscriptions before reporting Live, forwards interruption
and recovery notifications, and keeps the source guard in the observer task.
Failed startup, stop, cancellation and shutdown release the guard and report
Unavailable. Restart/reset reuses the same source name. Tracer Test reset clears
only the Test window and suppresses collection during replay; a failed reset keeps
Test unavailable until recovery. Applications supply feeds, not observer handlers
or duplicate binding registrations. The compatibility `start_correlation_observer`
entry point uses the same source handling; its returned task is caller-supervised.

See [continuous observation](continuous-observation.md) for retention, payload
policy, HTTP/SSE and Studio semantics. These observers capture externally initiated
domain/integration events as well as evidence for Tracer-created operations.

### Typed cross-context integration commands

```rust,ignore
impl IntegrationEvent for ProducerChanged {
    const EVENT_NAME: &'static str = "producer-changed";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some("producer");
}
// RentAfterChange implements IntegrationCommandMapper<ProducerChanged>.
bindings.register_integration_command::<ProducerChanged, _>(RentAfterChange);
```

The event type supplies producer identity once. Both `register_integration_handler`
and `register_integration_command` derive its address through the existing
`BoundedContext::integration_event_address` API, retaining the consumer runtime's
application and Test/Normal scope. `BOUNDED_CONTEXT: None` falls back to the
consumer context, preserving the existing bike-rental contracts.

The mapper convenience supplies the runtime command bus, consumer-owned durable
identity and `NatsRuntimeConfig::integration_retry_delay` (one second by default)
to the existing `IntegrationEventCommandHandler`. Its decoding, deterministic
operation identity, correlation/causation and retry/quarantine semantics are reused.
The mapped command still needs its normal typed command-handler registration.

Consumer identities remain consumer-context-owned and independent of event schema
version. Same-context names retain their existing identities. Foreign-producer
subscriptions use a bounded hash of the canonical address as their purpose, so
different producers with the same event name do not share a durable. Observation
codecs are keyed by producer, event name and schema version as well.

## Bike rental: one entry point, one flag

With JetStream NATS 2.12.1 or newer available:

```sh
cargo run --locked -p bike-rental
cargo run --locked -p bike-rental -- --tracer
```

Both starts use `tracer::bindings()` and run Normal command consumers, preserving
the released durable generation `2`. Changing a command payload schema does not
change its subscription. The example explicitly applies its idempotent demo
fixture to Normal state in both modes, as its former API entry point did.

Set `ROSTFREI_NATS_URL`. For Tracer also set `ROSTFREI_API_TOKEN` and the separately
configured `ROSTFREI_DISPATCH_TOKEN`; optional `ROSTFREI_INSPECTION_TOKEN` enables
production inspection. `ROSTFREI_API_ADDR` defaults to `127.0.0.1:1309`.
With `--tracer`, bike-rental configures both Test and Production observation feeds;
Production Observe requires the inspection token.
Existing `ROSTFREI_NATS_*_MAX_BYTES` limits remain supported. With Tracer disabled,
HTTP and correlation-observer workers are absent and HTTP tokens are unnecessary.

## Lifecycle and ownership

Writable startup requires explicit fresh split authority selection through
`with_fresh_completed_outcome_epoch`. Bike rental reads
`ROSTFREI_COMPLETED_OUTCOME_EPOCH` for both isolated Test and Normal configurations.
Use an owned disposable application/broker for qualification. Existing histories
remain legacy/read-only unless the operator performs an explicit fenced cutover;
an application name alone is not authority selection.

### Effective codecs and execution profile

`ApplicationBindings::new()` assembles default JSON aggregate codecs.
`with_event_codecs(Arc<AggregateCodecs>)` installs one frozen custom registry for
fixture validation, real execution, private-event dispatch and `simulate_handler`.
Register/replace codecs before sharing it and include fixture aggregate codecs.
Registering descriptors afterward does not mutate shared codec configuration.

`NatsRuntimeConfig.execution_profile` defaults explicitly to
`NatsExecutionProfile::TrustedCompatibility`: durable decisions, without configured
per-call policy admission. Configure the existing service path with:

```rust,ignore
config.execution_profile = NatsExecutionProfile::configured(
    move |builder| builder.authorizer(authorizer.clone())
        .audit_sink(audit.clone())
        .conflict_retry_policy(retry.clone())
        .rejection_codec(rejections.clone()),
    resolver, // Arc<dyn TrustedCommandContextResolver>
    sender,   // TrustedCommandContext, explicitly supplied by trusted ingress
);
```

The retained builder configuration is reapplied against the reconnected backend
after reset. Workers use the resolver, and senders use current outcome admission.
The host installs its effective event registry after the callback. Principal
identity is never inferred from actor attribution or HTTP capability tokens.
Configured Tracer replay requires real command republication and its actual PubAck,
followed by current winner admission; completed handlers are not reevaluated.

`NatsRuntime::store()` returns an owned current snapshot. Retained old snapshots
remain fenced after reset. `NatsRuntime::tracer_builder()` supplies current-history
and directory indirection for manually composed diagnostics; the application host
uses it automatically. History reads do not themselves verify authority health.

- `NatsRuntimeConfig::new(context, &bindings, subscription_generation)` derives
  consumers from registrations. Its explicit resource/consumer fields support
  application policies and retention of legacy names and delivery settings.
- `NatsRuntimeConfig::provision` / `NatsRuntime::provision` are explicit operator
  operations. The configuration derives a complete resource plan from the supplied
  bindings, including the domain-event durable when integration mappings need it.
  Independent `config.provision(&connection)` followed by
  `builder.with_runtime_config(config).connect_and_start()` uses the same plan as
  combined provisioning and Test reset. Rebuild the configuration when changing
  bindings; its public resource policies remain adjustable.
  `connect` / `connect_and_start` verify existing resources without
  provisioning or resetting. `with_initial_fixture` is explicit idempotent
  initialization, distinct from Test reset.
- `provision_and_start` with a default Test fixture explicitly resets that Test
  scenario before readiness. Preparation verifies all consumers and subscribes
  observers before spawning the owned group. Failed preparation starts no group.
- `NatsApplication` continuously supervises its workers and HTTP server. An exit
  stops the owned group; `run_until` and `shutdown` surface the underlying failure.
  Shutdown ends streaming observation responses, awaits HTTP cleanup and stops
  workers. Dropping the host requests the same cleanup. The supplied managed NATS
  connection is caller-owned; the host does not drain other users' connections.
- A reset takes an exclusive process-local submission gate, waits for active
  transported commands, stops the group, deletes only configured Test streams,
  recreates both journal and outcome-state streams, reconnects and rebuilds the
  processor, service, transports and observers, registers fixture event identities
  **before** replay, then restarts. Domain history and outcomes reset together. Any failure keeps
  publication unavailable; `start_workers` cannot bypass a failed reset.
- Framework observers validate event identity, correlation and causation against
  delivery headers and envelopes. Integration events use the registered typed
  codecs, including provenance-derived identity checks. Evidence uses actual
  committed events; retired simulation requests return 404 without publication.
- Production Dispatch/inspection is attached only with an explicitly supplied
  Normal `NatsRuntime` through `TracerConfig::with_dispatch_runtime`. The host
  supervises that runtime too. Test reset cannot select Normal resources.
- Shutdown retains broker data. Reset is the explicit destructive operation.

Handler simulation remains diagnostic-only: no operation, durable outcome, command
publication or synthetic MessageSeries. Preview remains unavailable. The known
direct-runtime-reset diagnostic-health boundary is still deferred: Tracer-mediated
reset gates diagnostics, but a direct failed `NatsRuntime::reset` does not update
Tracer's separate diagnostic health flag. Current readable history is not proof of
an intact writable authority. See [diagnostic limitations](tracer-preview-retirement.md).

### Submission admission during teardown

`NatsRuntime::stop_workers` closes runtime-scoped transport admission before
waiting for active submissions. Explicit application `shutdown`, a `run_until`
shutdown signal, application Drop and supervised failure close both Test and
attached Normal admission before HTTP/task cleanup. Raw runtime Drop also closes
retained transports before aborting workers. New invocations return `Unavailable`
without publishing a command, even when another owner retains the transport.

Already admitted invocations continue through the existing response wait during
graceful stop/shutdown. A command already published to NATS can still execute if
its caller stops waiting; teardown does not retract durable messages. Raw runtime
Drop cannot await graceful completion; use explicit shutdown for that. A stopped
raw runtime can be explicitly restarted; application shutdown is terminal for its
owned runtimes. Reset retains its exclusive submission gate and Test-only scope.

## Current isolation boundary and related issues

One host owns the Test messaging namespace for its application and one bounded
context's event store. Use a dedicated broker or a unique `ApplicationName` for
each independent development host/process. External publishers must not submit
into that owned Test namespace during reset. The gate coordinates host
submissions, not independent processes or arbitrary broker publishers.

This uses the existing unqualified `TrafficScope::Test`. Framework-owned run IDs
and same-logical-application cross-process isolation remain #68. The typed
registration foundation and explicit consumer generation support #101, but its
full topology/retention-aware compatibility diagnostics are separate work.

## Measurement

Physical Rust lines, including imports, comments and blanks:

| Application-owned file | Before (`1d7d861`) | After |
| --- | ---: | ---: |
| `bike_rental_nats/mod.rs` | 1,061 | 516 |
| `bin/bike-rental-api.rs` | 119 | 94 |
| `tracer.rs` | 34 | 59 |
| **Total** | **1,214** | **669** |

Reduction: **545 lines (45%)**, including both continuous-observation feeds.
Against the issue's v0.0.5-alpha baseline of 1,211 lines, the reduction is 542 lines.
The retained code includes business
integration mappings, resource/environment policy and compatibility accessors
used by existing integration/inspection tests; it is not all host boilerplate.

The standalone consumer has **60 composition/startup lines**: 43 in `main.rs`
plus 17 in its binding functions, excluding domain code and fixture/test JSON.

See [acceptance evidence](tracer-host-acceptance.md) and run:

```sh
python3 scripts/test_nats.py -- python3 scripts/check_tracer_host.py
python3 scripts/test_nats.py -- python3 scripts/check_bike_rental_host.py --studio
```

The first uses an external package with Cargo Git dependencies pinned to the
checkout's committed revision, and verifies all Rostfrei crates resolved from
that revision. Use `--working-tree` only for pre-commit development. The second
needs the Studio dependencies, Node 24 and Chrome (`CHROME_BIN` if necessary).
