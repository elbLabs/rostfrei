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
    .integration_source::<BillingChanged>(
        billing_context,
        rostfrei::IntegrationEventOrder::<BillingChanged>::latest(
            "billing-account",
            |event| event.source_version,
        ),
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

Integration inputs must implement `IntegrationEvent`. Their producer context and
ordering policy are required; `.build()` rejects an unspecified policy rather than
guessing business ordering from delivery attempts, message IDs, or broker sequences.
The producer may be another bounded context in the same application and traffic scope.

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

For integration events, declare one of:

- `IntegrationEventOrder::consecutive("source", position)`: each key's source starts
  at version 1 and requires contiguous deltas. A gap rejects materialization.
- `IntegrationEventOrder::latest("source", position)`: each fact completely
  supersedes smaller versions for that key/source. This permits skipped versions;
  use it only when the transformation has that meaning.

The source name is scoped by producer context. Several event types may share it
only if they describe the same logical version sequence and compatible policy.
Separate sources need separate names; versions must be positive and authoritative
for the model key. A provider webhook ID or timestamp is not automatically such a
version. The application still owns freshness and ordering meaning.

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

The worker uses one independent domain durable per model and one durable per
integration binding, named deterministically from the model/schema and source.
Two models can subscribe to the same event without sharing delivery progress.
It handles only committed domain events and integration events. Consumer tasks
are owned by the worker future, and are cancelled on shutdown or worker failure.
Restart/redelivery handles writes whose outcome became uncertain during shutdown.

Successful materialization is awaited before ACK. Domain failures compose with
the existing domain-consumer blocking/retry contract. Integration failures can
be quarantined immediately, or after retry exhaustion, which terminally advances
delivery without materialization. See [quarantine and recovery](read-models.md#integration-quarantine-and-recovery):
repairing storage alone does not replay a quarantined event. Republish with a fresh
message ID and the original logical source version, or publish a refreshed latest
authoritative fact. A normal reader never treats consumer progress as freshness.

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
