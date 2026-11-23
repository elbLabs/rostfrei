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

### Studio operational inventory

Studio reads one Tracer API endpoint. Each optional inventory is a `snapshotHref`
in that endpoint's **inspection** catalog. These GETs also work on an
inspection-only router; Control and Dispatch credentials do not authorize them.

| Catalog entry | Source | Question it answers |
| --- | --- | --- |
| `infrastructure` | Configured JetStream accounts | How much memory/storage is in use? Which streams, KV buckets and consumers are visible, paused or backed up? |
| `servers` | Configured NATS monitoring endpoints | Does this server report healthy? What are its process RSS, uptime, version and reported JetStream limits? |
| `workloads` | Explicitly mapped container memory source | What memory is charged to the read model's container and what is its limit? See [projection storage and workload bindings](read-models/nats.mdx). |
| `domainInventory` | Linked model, registry and explicit projection declarations | Which contexts, aggregates, entities, policies, operations and read models does this host declare? |

```rust,ignore
use rostfrei::tracer::broker_operations::{NatsBrokerOperations, NatsBrokerSource};

let inventory = bindings.studio_domain_inventory()?;
let accounts = NatsBrokerOperations::new([
    NatsBrokerSource::connected("Ordering", ordering_connection.clone())?,
    NatsBrokerSource::configured("Billing", billing_inspection_connection_config)?,
])?;
let tracer = TracerConfig::new(listener, http)
    .with_domain_inventory(inventory)
    .with_broker_operations(Arc::new(accounts));
```

The labels identify operator-configured connections/accounts, not discovered
servers. A connection may fail over between cluster nodes. Grant account-wide
visibility only to an appropriate audience: even a read-only listing can expose
another application's stream, bucket and durable names. Use a dedicated reader
credential with `$JS.API.INFO`, `$JS.API.STREAM.LIST`, `$JS.API.CONSUMER.LIST.<stream>`
and inbox replies; inspection needs no data-publish, message-read, provision or
reset permission. Refresh takes a bounded sample; sections can be unavailable,
timed out or truncated independently. Account memory/file storage and finite
limits are broker figures. Do not add KV bytes to their backing stream's bytes,
or treat an account quota as the memory limit of a container.

In **Infrastructure**, open a configured broker source to explore its streams
and KV buckets. Open a stream to see its consumers; select the current source,
stream or another resource to inspect its readings. Configured NATS nodes are
shown separately because a client connection does not identify a server.
In **Read models**, selecting a projection shows its policy and any mapped
container memory. A link leads to its declared KV store, where Studio shows
broker usage only when both bucket and backing stream match on the named source.
A declaration does not prove the store exists, and a partial listing cannot
establish absence. Close a details overlay with **Esc** or its close button;
exploring or inspecting cards does not take another sample.

To sample individual nodes, pass a `server_monitoring::NatsServerMonitor` with
one `NatsServerMonitorSource::new(label, base_url)?` per **server** to
`with_server_monitoring`. Configure a private HTTP(S) monitoring endpoint, not
a cluster load balancer. Only the host contacts `/healthz` and `/varz`; each
probe has its own outcome. Studio sees labels and selected metrics, never raw
monitoring responses. Bike-rental accepts a single `ROSTFREI_NATS_MONITOR_URL`.

The domain inventory is built without a broker read. `compiledModelAvailable`
distinguishes a supplied `domain_model!` projection from registry metadata alone.
Only registered aggregates have `aggregateType`; a compiled root or entity does
not imply an independently persisted record. Use IDs for joins, not labels.
In Studio, open **Domain** and choose an aggregate. Its entities and events
share one canvas. Hover or focus a card to trace its connections. Solid lines
show membership; dashed lines show explicit field references. Select the card
to see its fields and declared Rust types.
For a command, choose its bounded
context and expand **Registered commands**, then its version. These are modeled
payload types: `PaymentId` is a type name, not a claim about the JSON encoding
or a link to a payment aggregate. Closing the overlay keeps your place.

**Read-model storage is an explicit binding.** A `ReadModel` derive or a KV bucket
alone does not establish that a running service uses that storage. At the
composition point that configures the projection, supply the actual model policy:

```rust,ignore
use rostfrei::tracer::domain_inventory::ReadModelDeclaration;

let model = ReadModelDeclaration::from_nats_config::<OrganizationAccess>(&policy)
    .ok_or("read-model declaration and storage config do not match")?;
let inventory = bindings.studio_domain_inventory()?
    .with_read_model(model, Some("Ordering".into()));
```

The source label binds the stream to a particular account; names can repeat
across accounts. For a different backend, `ReadModelDeclaration::for_model`
supplies the logical declaration without claiming a KV budget. Live usage,
configured capacity, and the maximum size of one stored value are different
measurements. The inventory never enumerates stored domain instances or KV values.

**Across microservices:** an application-linked catalog describes the code and
registrations supplied by that host. A bounded context is not a service replica.
Point a Studio deployment at each service's Tracer for its commands and domain
declarations, or compose the read-only inventory explicitly on an inspection
host. Use separate credentials and Studio origins for independent services
(saved development runs are browser-local). One inspection host can sample
several configured brokers. For retained correlations **within one application**,
register the participating Normal context stores on a shared connection as
described below. Tracer does not crawl peers, merge unrelated correlation IDs,
or route commands to services inferred from stream names. The standalone
`TracerBuilder` accepts the same inventory and reader without starting Test
workers; the `NatsApplication` convenience host still owns a Test runtime.

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
Attaching or replacing observation preserves running application workers and their
in-flight deliveries. Reset and shutdown still stop the owned observer and workers.

See [continuous observation](continuous-observation.md) for retention, payload
policy, HTTP/SSE and Studio semantics. These observers capture externally initiated
domain/integration events as well as evidence for Tracer-created operations.

### Retained correlation inspection

Attaching a Normal runtime with `TracerConfig::with_dispatch_runtime` automatically
registers its domain store and the application's shared Normal integration stream
for on-demand correlation reads. There is no projector flag or background index.
Add other contexts with their existing `NatsEventStoreConfig`:

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
the attached Normal runtime's configuration,
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

With JetStream NATS 2.12.1 or newer available:

```sh
cargo run --locked -p bike-rental
cargo run --locked -p bike-rental -- --tracer
```

Both starts use `tracer::bindings()` and run Normal command consumers, preserving
the released durable generation `2`. Changing a command payload schema does not
change its subscription. Both modes seed Normal state through idempotent
`RegisterBicycle` commands. Named fixtures are reserved for isolated Test state.

Set `ROSTFREI_NATS_URL`. For Tracer also set `ROSTFREI_API_TOKEN` for isolated Test
control. Set `ROSTFREI_INSPECTION_TOKEN` to enable production inspection, and set
`ROSTFREI_DISPATCH_TOKEN` only if production command dispatch is wanted. Use
distinct tokens. `ROSTFREI_API_ADDR` defaults to `127.0.0.1:1309`.
The example includes its compiled domain inventory. Set `ROSTFREI_INSPECT_BROKER=true`
to additionally expose metadata from its NATS account to the inspection capability.
Bike-rental does not declare a KV-backed read model; it will not invent one for Studio.
With `--tracer`, bike-rental configures both Test and Production observation feeds.
Reading production traffic requires the inspection token.
Existing `ROSTFREI_NATS_*_MAX_BYTES` limits remain supported. With Tracer disabled,
HTTP and correlation-observer workers are absent and HTTP tokens are unnecessary.

## Lifecycle and ownership

Writable startup requires explicit fresh split authority selection through
`with_fresh_outcomes()`, without an epoch label. Bike rental selects this layout
for both Test and Normal configurations by default. Its optional
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
- Production Dispatch uses an explicitly supplied Normal `NatsRuntime` through
  `TracerConfig::with_dispatch_runtime`; the host supervises that runtime too.
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

One host owns the Test messaging namespace for its application and one bounded
context's event store. Use a dedicated broker or a unique `ApplicationName` for
each independent development host/process. External publishers must not submit
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
