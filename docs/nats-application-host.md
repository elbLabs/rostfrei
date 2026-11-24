# Application-linked NATS host

Enable `rostfrei`'s `nats` and `tracer` features and import the host from
`rostfrei::tracer` (see [dependency setup](getting-started.mdx#one-framework-dependency)).
`ApplicationBindings` is the executable composition: register aggregate fixture
codecs, command instances and rejection mappers once. Those same registrations supply catalog metadata,
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
    .with_fresh_outcomes()?
    .with_default_test_fixture(fixture)
    .with_tracer(Some(tracer))
    .provision_and_start().await?;
host.run_until(shutdown_signal).await?;
```

`TracerConfig` accepts an already-bound TCP listener and existing HTTP capability
configuration. Payloads are redacted unless a policy is explicitly supplied.
Catalog v1 advertises fixture v1 and behavioral-definition v2 discovery links.
Repository validation, execution and evidence use the existing Tracer engines.

The catalog combines registrations from the configured Test contexts. Commands
route to the exact context; aggregate reads and fixture writes route to the
runtime that registered the exact aggregate type. Installing an application-wide
codec registry does not expose aggregates from unhosted contexts. A command
schema advertises production dispatch only if an attached Normal runtime
registers it. A fixture can span contexts, but replay is not atomic across their
event stores.

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
checks each feed's application against its runtimes. One feed serves all attached
contexts in its scope. Production observation requires an attached Normal runtime.
HTTP discovery and access retain the existing control (Test) and read-only
inspection (Production) capabilities.

The framework registers a source named for each runtime's bounded context,
establishes both broker subscriptions before reporting Live, forwards interruption
and recovery notifications, and keeps the source guard in the observer task.
Failed startup, stop, cancellation and shutdown release the guard and report
Unavailable. Restart/reset reuses the same source name. Tracer Test reset clears
only the Test window and suppresses collection during replay; a failed reset keeps
Test unavailable until recovery. Applications supply feeds, not observer handlers
or duplicate binding registrations. The compatibility `start_correlation_observer`
entry point uses the same source handling; its returned task is caller-supervised.
Attaching or replacing observation preserves running application workers and their
in-flight deliveries. Reset stops Test observers and workers; shutdown stops all
owned observers and workers.

See [continuous observation](continuous-observation.md) for retention, payload
policy, HTTP/SSE and Studio semantics. These observers capture externally initiated
domain/integration events as well as evidence for Tracer-created operations.

### Retained correlation inspection

Each Normal runtime attached with `TracerConfig::with_dispatch_runtime` automatically
registers its domain store for on-demand correlation reads. The contexts share the
application's Normal integration stream. There is no projector flag or background
index. Add read-only contexts with their existing `NatsEventStoreConfig`:

```rust,ignore
// billing_events describes the already-provisioned Normal billing event store.
let tracer = TracerConfig::new(listener, http)
    .with_dispatch_runtime(normal_runtime)
    .with_production_event_store(billing_events);
```

An inspection-only host can register histories without a dispatch runtime:

```rust,ignore
let tracer = TracerConfig::new(
    listener,
    HttpConfig::inspection_only(inspection_token)?,
)
.with_production_event_store(ordering_events)
.with_production_event_store(billing_events);
```

Each config must match the host application and Normal traffic scope, including
the deployed authority epoch. Conflicting configs for the same context are
rejected. These registrations connect read-only to existing resources; they do
not provision resources or start application workers for those configs. The
history handle uses `NatsEventStore::connect_history`, validating and pinning the
journal's historical authority without opening live result state. Missing or
expired result details do not erase retained events; replacing the journal
invalidates the attached history handle. The shared integration topology uses
the attached Normal runtime configuration,
or the host application's Normal defaults for inspection-only registration.
Source credentials need the [retained reader's permissions](continuous-observation.md#retained-correlation-inspection).

For manual composition, use
`TracerBuilder::with_correlation_history(Arc<dyn CorrelationHistory>)` and
`with_production_history(context, Arc<dyn EventHistory>)`. The latter supplies
aggregate history for anchor and explicitly included payload reads.

Retained inspection uses the production inspection HTTP capability and follows
[source retention](messaging/subjects.mdx#retention). See
[retained correlation inspection](continuous-observation.md#retained-correlation-inspection)
for catalog discovery, payload policy, cutoffs, and errors. Its
[fresh-namespace requirement](messaging/subjects.mdx#fresh-deployments) is separate
from outcome-epoch selection.

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

One address has one durable, so it takes exactly one integration handler.
`NatsRuntimeConfig::new` rejects a second handler for the same address, including
another schema version of the same contract or an explicitly declared own context.
To accept several schemas, register one `MessageHandler<IntegrationEventAddress>`
that decodes each of them (see [schema upgrades](messaging-upgrades.md#payload-schema-upgrades)).
Typed version-aware decoding is tracked in [#111](https://github.com/elbLabs/rostfrei/issues/111).

## Bike rental: one entry point, one flag

With JetStream NATS 2.12.1 or newer available, choose a fresh
`ROSTFREI_APPLICATION` namespace and keep it for subsequent starts:

```sh
cargo run --locked -p bike-rental --bin bike-rental-api
cargo run --locked -p bike-rental --bin bike-rental-api -- --tracer
```

Both modes run Normal runtimes for Rental (`rental`), Finance (`finance`), and
Workshop (`workshop`) and seed Normal Rental state through idempotent
`RegisterBicycle` commands. Command durables retain subscription generation `2`,
independent of payload schemas. Each context has its own bindings, workers, and
journal/result pair. The contexts share one set of command, command-response,
integration-event, and quarantine streams per application and traffic scope.
Cross-context work goes through integration events and commands, not a shared
transaction or a read of another context's private journal.

`--tracer` adds a separate Test runtime for each context. One logical Tracer
runtime and catalog route commands and fixtures to the owning contexts. To
compose a host this way, register additional Test configs and bindings with
`NatsApplicationBuilder::with_context(config, bindings)` and call
`TracerConfig::with_dispatch_runtime` once per Normal context. A Tracer reset
rebuilds the whole Test scenario from registered fixtures, including its shared
Test streams and each context's journal/result pair. It leaves Normal workers
and data untouched.

Set `ROSTFREI_NATS_URL`. For Tracer also set `ROSTFREI_API_TOKEN` and the separately
configured `ROSTFREI_DISPATCH_TOKEN`; optional `ROSTFREI_INSPECTION_TOKEN` enables
production inspection. `ROSTFREI_API_ADDR` defaults to `127.0.0.1:1309`.
With `--tracer`, bike-rental configures both Test and Production observation feeds;
Production Observe requires the inspection token.
Existing `ROSTFREI_NATS_*_MAX_BYTES` limits remain supported. With Tracer disabled,
Test runtimes, HTTP and correlation-observer workers are absent and HTTP tokens
are unnecessary.

The old single-context deployment is not migrated in place. Use a fresh
application namespace even if the old example already used label-free storage;
changing only the outcome epoch is not a migration. Leave the old streams and
durables intact: the new host does not adopt their history, and existing retries
remain tied to the old authority. See the [example setup](../examples/bike-rental/README.md#run-locally).

## Lifecycle and ownership

Writable startup requires explicit split authority selection for each context.
Unsplit runtime plans are rejected before provisioning or resetting streams.
To provision label-free storage for a group, select
`NatsRuntimeConfig::with_fresh_outcomes()` on each configuration you supply.
Bike rental selects it for both Test and Normal by default. Its optional
`ROSTFREI_COMPLETED_OUTCOME_EPOCH` setting reopens existing named-epoch stores.
Use an owned disposable application/broker for qualification. Existing legacy
authorities remain read-only unless the operator performs an explicit fenced
cutover; an application name alone is not authority selection. See also the
[subject deployment requirements](messaging/subjects.mdx#fresh-deployments).

### Effective codecs and execution profile

`ApplicationBindings::new()` assembles default JSON aggregate codecs.
`with_event_codecs(Arc<AggregateCodecs>)` installs one frozen custom registry for
fixture validation, real execution, private-event dispatch and `simulate_handler`.
Register/replace codecs before sharing it and include fixture aggregate codecs.
Registering descriptors afterward does not mutate shared codec configuration.
The installed registry is complete: `register_aggregate` rejects an aggregate
without a codec in it (`UnknownAggregateCodec`) instead of falling back to
generated JSON. Pass the same registry to `ReadModels::with_event_codecs`.

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

`NatsRuntime::store()` returns an owned current store handle. Retained old handles
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
  If attaching dispatch observers fails, a later caller-owned restart uses the
  observer settings the dispatch runtimes had before the failed startup.
- `NatsApplication` continuously supervises its workers and HTTP server. An exit
  stops the owned group; `run_until` and `shutdown` surface the underlying failure.
  Shutdown ends streaming observation responses, awaits HTTP cleanup and stops
  workers. Dropping the host requests the same cleanup. The supplied managed NATS
  connection is caller-owned; the host does not drain other users' connections.
- A reset takes an exclusive process-local submission gate, waits for active
  transported commands, stops the Test group, deletes only configured Test streams,
  recreates the shared messaging streams and each Test context's journal and
  outcome-state streams, and rebuilds the processors, services, transports and
  observers. It registers fixture event identities **before** replay, then
  restarts the group. Any failure keeps Test publication unavailable;
  `start_workers` cannot bypass a failed reset.
- Framework observers validate event identity, correlation and causation against
  delivery headers and envelopes. Integration events use the registered typed
  codecs, including provenance-derived identity checks. Evidence uses actual
  committed events; retired simulation requests return 404 without publication.
- Production Dispatch uses explicitly supplied Normal `NatsRuntime`s through
  `TracerConfig::with_dispatch_runtime`; the host supervises those runtimes too.
  Retained inspection also accepts read-only `with_production_event_store`
  registrations without Dispatch. Test reset cannot select Normal resources.
- Shutdown retains broker data. Reset is the explicit destructive operation.

Handler simulation remains diagnostic-only: no operation, durable outcome, command
publication or synthetic MessageSeries. Preview remains unavailable. The known
direct-runtime-reset diagnostic-health boundary is still deferred: Tracer-mediated
reset gates diagnostics, but a direct failed `NatsRuntime::reset` does not update
Tracer's separate diagnostic health flag. Current readable history is not proof of
an intact writable authority. See [diagnostic limitations](handler-simulation.md).

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

## Isolation

One host owns the Test messaging namespace for its application and the configured
Test contexts' event stores. Use a dedicated broker or a unique `ApplicationName`
for each independent development host/process. External publishers must not submit
into that owned Test namespace during reset. The gate coordinates host
submissions, not independent processes or arbitrary broker publishers.

## Integration checks

```sh
cargo xtask test -- cargo xtask acceptance tracer-host
cargo xtask test -- cargo xtask acceptance bike-rental-host --studio
```

The first uses an external package with Cargo Git dependencies pinned to the
checkout's committed revision, and verifies all rostfrei crates resolved from
that revision. Use `--working-tree` only for pre-commit development. The second
needs the Studio dependencies, Node 24 and Chrome (`CHROME_BIN` if necessary).
