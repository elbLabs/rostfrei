# Event-driven read models

Declare the data your query needs, register event transformations, and let Rostfrei
load, checkpoint, and save it. The runtime handles CAS retries and persists source
progress together with the business value. Queries receive a read-only handle.

The complete runnable example is
[`examples/read_model`](../crates/rostfrei-nats/examples/read_model/main.rs).
The declaration and registration are in
[`application.rs`](../crates/rostfrei-nats/examples/read_model/application.rs).

## Declare the model

Install macro support once at the application crate root, as for other Rostfrei
declarations:

```rust
rostfrei::install_macro_support!();
```

```rust
#[derive(Default, serde::Serialize, serde::Deserialize, rostfrei::ReadModel)]
#[read_model(id = "organization-access", version = 1)]
struct OrganizationAccess {
    members: u64,
    demo: bool,
    paid: bool,
}
```

`ReadModel` requires an ordinary struct, a validated lowercase kebab-case ID, a
positive schema version, `Default`, and Serde serialization/deserialization.
`Default` initializes a missing key. The struct contains business data only;
revisions and per-source checkpoints belong to the runtime's persistence envelope.
The derive declares metadata; it performs no network or provisioning operations.
Checkpoint metadata counts toward the configured value-size limit. Models joining
an unbounded number of aggregate identities should partition their keys accordingly.

## Register transformations

```rust
let model = read_models
    .register::<OrganizationAccess>()
    .from_aggregate::<Organization>()
    .try_on_domain_event::<MemberJoined>(
        |event| event.organization_id.clone(),
        |view, _event| {
            view.members = view.members.checked_add(1).ok_or_else(|| {
                rostfrei::ReadModelProcessingError::Transformation(
                    "member count overflow".to_owned(),
                )
            })?;
            Ok(())
        },
    )
    .on_domain_event::<DemoChanged>(
        |event| event.organization_id.clone(),
        |view, event| view.demo = event.active,
    )
    .on_integration_event::<BillingChanged>(
        |event| event.organization_id.clone(),
        |view, event| view.paid = event.paid,
    )
    .build()
    .await?;
```

The first closure selects one key. The second transforms the value for that key.
Use `on_domain_event` / `on_integration_event` for infallible transformations and
their `try_on_*` variants when validation can fail. All transformations are
synchronous and may be evaluated again after a conflict or on rebuild. They must
be deterministic and free of external side effects. Fetch externally authoritative
facts outside this callback, then deliver a versioned integration event.

`from_aggregate::<A>()` selects the owner of subsequent domain registrations.
Switch it to register another aggregate in the same bounded context. The compiler
checks that each domain input is both a `DomainEvent` and a member of that
aggregate's event set. Domain key selectors also expose committed metadata:

```rust
|event| event.recorded().stream_id().aggregate_id().as_str().to_owned()
```

Use that form when an event payload does not repeat its aggregate ID. The selector
otherwise dereferences to the typed event, so ordinary `event.organization_id`
field access works.

Integration inputs must implement `IntegrationEvent`. Broker ordering is automatic;
an ordinary event does not need a business version or an `integration_source` call.
Declare a cross-context producer once with its public event contract:

```rust
impl rostfrei::IntegrationEvent for BillingChanged {
    const EVENT_NAME: &'static str = "billing-changed";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some("billing");
}
```

When `BOUNDED_CONTEXT` is absent, the model's bounded context is used. For an
existing unannotated contract, `.integration_context::<E>(producer_context)`
overrides routing while retaining broker order. Declared producers are checked by
the typed integration bus and read-model registration. Sources stay in the same
application and traffic scope, even when produced by another bounded context.

Commands and queries do not satisfy these registration bounds. Compile-failure
tests verify rejection of both input families, foreign aggregate event types,
invalid model declarations, and mutation through a `ReadModelReader`.
Duplicate event bindings, model IDs in one registry, incompatible contexts, and
cross-Normal/Test sources are also rejected.

## Source progress and catch-up

For domain events, the runtime tracks each `(bounded context, aggregate type,
aggregate ID)` independently **inside each key's saved state**. Already-applied
versions are no-ops. If a delivered version jumps ahead, it loads authoritative
history and applies relevant registered events in stream-version order through
that version. Events with no handler, and events routed to another key, do not
change this key. The entire catch-up value and checkpoint commit in one CAS.
This makes filtered events and out-of-order concurrent dispatch safe without
requiring application-owned checkpoint fields or no-op handlers.

An unavailable or incomplete history prevents the write. Loading a full aggregate
history for a gap has a cost; the usual next-version path only applies the supplied
event. Long histories can therefore increase materialization latency even though
queries remain a single KV read.

### Default: broker ordering

All of a model's registered integration subjects share one ordered NATS durable
with one outstanding delivery (`MaxAckPending=1`). This preserves broker order
across event types and producing contexts, including while an earlier message is
waiting for retry. The worker rejects incompatible consumer settings.

Each key's saved state includes the latest successfully materialized broker source
sequence. It commits atomically with the value. Redelivery uses the original source
sequence, so an uncertain write that actually committed is not applied twice.
Consumer delivery-attempt sequences and payload schema versions are not checkpoints.
Gaps in broker source sequences are normal when unrelated subjects are filtered out.

Broker order is arrival order, not business chronology. A late fact published as a
new message can update the view. Likewise, republication after the broker's duplicate
window or a quarantine redrive has a new source sequence and can apply again. The
default does not deduplicate arbitrary new publications by logical business identity.
Use idempotent state updates, authoritative reconciliation, or opt into business
versions when that distinction matters.

Custom adapters invoking the exposed handlers must provide one ordered, stable
integration source stream per model and preserve the single-in-flight/retry
contract. Independent concurrent deliveries cannot safely use a broker high-water
mark. NATS read-model workers enforce this contract. There is no total ordering
between the separate domain-event and integration-event streams.

### Optional: business ordering

For an event carrying an authoritative per-key business version, override the
default with `integration_source`:

```rust
.integration_source::<VersionedBillingChanged>(
    billing_context,
    rostfrei::IntegrationEventOrder::<VersionedBillingChanged>::latest(
        "billing-account",
        |event| event.source_version,
    ),
)
```

Choose one of:

- `IntegrationEventOrder::consecutive("source", position)`: each key's source starts
  at version 1 and requires contiguous deltas. A gap rejects materialization.
- `IntegrationEventOrder::latest("source", position)`: each fact completely
  supersedes smaller versions for that key/source. This permits skipped versions;
  use it only when the transformation has that meaning.

Named business checkpoints use a separate namespace from broker checkpoints.
The source name is scoped by producer context. Several event types may share it
only if they describe the same logical version sequence and compatible policy.
Separate sources need separate names; versions must be positive and authoritative
for the model key. A provider webhook ID or timestamp is not automatically such a
version. `IntegrationEventOrder::broker()` explicitly selects the default when
needed. The application still owns freshness and ordering meaning.

Concurrent handlers reload the state and reevaluate their transformation after a
CAS conflict. There are eight attempts per delivery, then a retryable error. A
timeout may have committed: the persisted checkpoint resolves redelivery without
reapplying the transformation. Errors returned by a transformation do not persist
its partial in-memory changes.

## Bind NATS and start the worker

Infrastructure remains explicit, using the existing managed connection:

```rust
let policy = NatsReadModelConfig::for_model::<OrganizationAccess>(&context)?;
provision_read_model(connection.jetstream(), &policy).await?;

let backend = NatsReadModelBackend::new(connection.jetstream().clone())
    .with_model_config(policy)
    .with_event_store_config(event_store_config.clone());
let read_models = ReadModels::new(context, backend);
// Register handlers and call .build().await? as above.

let options = NatsReadModelConsumerOptions::default();
provision_read_model_consumers(
    connection.jetstream(), &model, &event_store_config, &topology, &options,
).await?;
let worker = NatsReadModelWorker::connect(
    connection.jetstream().clone(), &model, &event_store_config, &topology, &options,
).await?;
worker.run_until_shutdown(shutdown_receiver).await?;
```

Provision the authoritative event store and application messaging topology through
their existing APIs. The backend defaults to the corresponding scoped event-store
and read-model policies; use the shown overrides for nondefault limits/configuration.
`.build()` opens/verifies resources and does not create them.

The worker uses one independent domain durable and one integration durable per
model. The latter filters exactly the registered integration subjects and processes
them sequentially. Durables are named deterministically from the model/schema.
Two models can subscribe to the same event without sharing delivery progress.
It handles only committed domain events and integration events. Consumer tasks
are owned by the worker future, and are cancelled on shutdown or worker failure.
Restart/redelivery handles writes whose outcome became uncertain during shutdown.

Successful materialization is awaited before ACK. Domain failures compose with
the existing domain-consumer blocking/retry contract. Integration failures can
be quarantined immediately, or after retry exhaustion, which terminally advances
delivery without materialization. See [quarantine and recovery](read-models.md#integration-quarantine-and-recovery):
repairing storage alone does not replay a quarantined event. In broker mode a
redrive is a new, later publication; order-sensitive deltas may require rebuilding
or reconciliation instead. With business ordering, retain the original logical
source version when redriving, or publish a refreshed latest authoritative fact.
A normal reader never treats consumer progress as freshness.

## Query and rebuild

```rust
let reader = model.reader();
let access = reader.read(&ReadModelKey::new("org-123")?).await?;
```

`ReadModelReader<M>` is cloneable and only returns `Option<M>`. It exposes neither
the mutable store nor revision tokens. Pass it into an application query handler;
there is no command/query subscription API.

To rebuild, provision a new model generation and call `model.dispatch_domain` on
committed events loaded from `EventHistory`, then refresh integration-source facts
through their registered integration binding/consumer. Coordinate source catch-up
and query cutover explicitly, as described in the storage guide. Changing handler
semantics or adding handlers for earlier events requires a rebuild: existing
checkpoints deliberately prevent those events from being reapplied.

Runtime persistence uses `ReadModelState<M>` inside the versioned codec, including
required internal envelope metadata. It is incompatible with a value-only record
or the low-level example's manually checkpointed schema. Use a fresh named bucket
and rebuild when adopting the runtime; do not manufacture or copy KV revisions.
Model schema version changes also need the usual deliberate migration/rebuild.
Changing ordering policies is also a generation/rebuild decision. Broker and
business checkpoint numbers cannot be translated into each other. Deploying the
ordered-worker topology replaces the earlier per-integration-type durables; stop
old workers and retire those old durables during cutover.
When recreating authoritative history, including an isolated Test reset, reset or
rebuild the associated read-model buckets and consumer progress as part of the
same lifecycle. A source version from a previous history generation cannot serve
as progress for the replacement history.
Deleting/expiring a key also removes its checkpoints and contributions from other
sources. Rebuild or refresh every contributing source before treating such a
recreated joined model as complete; a single new event only repairs its own source.

The runtime updates one key atomically per event. Cross-key materialization,
retention/freshness policy, and external-source reconciliation remain application
contracts. Use the [low-level storage API](read-models.md) when those contracts
require custom orchestration.

## Run the example

```sh
python3 scripts/test_nats.py -- cargo run --locked -p rostfrei-nats --example read_model
```

The example commits two relevant domain events with an ignored event between them,
publishes a billing integration event from another bounded context, starts the
registered consumers, waits for materialization, and reads through a typed query.
