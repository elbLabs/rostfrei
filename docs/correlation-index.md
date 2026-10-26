# Correlation index and anchored views

The correlation index answers "which messages carried correlation R1" across
every bounded context of an application, durably and for traffic that Tracer
did not submit. Anchored views list every correlation that touched one
aggregate. See [ADR 0045](adr/0045-correlation-index-and-anchored-views.md) for
the decision and its limits.

## Enable the projector

Each bounded-context runtime projects its own committed domain events and the
integration events it publishes. Enable it on the runtime configuration or on
the application host:

```rust,ignore
let normal = NatsRuntimeConfig::new(context, &bindings, 1)?
    .with_correlation_index()?;
let runtime = NatsRuntime::provision(connection.clone(), normal, bindings.clone()).await?;

// or, for the host's own runtime:
let host = NatsApplication::builder(context, connection, bindings)?
    .with_correlation_index()?
    .provision_and_start().await?;
```

`provision` creates the bucket together with the runtime's other resources;
`connect` only verifies it. The projector is a supervised worker: it starts and
stops with the runtime's other workers, and a stopped projector stops the host.
A Tracer host attaches the index of its Normal dispatch runtime automatically.

The projector reads each source through an ordered consumer named
`correlation-index--<domain|integration>--<context>--<uuid v7>`. It deletes the
consumer when it stops, fails or is aborted. A consumer left behind by a lost
process is removed by the server after 30 seconds of inactivity.

Without the host, run the projector and reader directly:

```rust,ignore
let index = NatsReadModelConfig::correlation_index(&application, TrafficScope::Normal)?;
provision_read_model(connection.jetstream(), &index).await?;
let projector = NatsCorrelationIndexProjector::connect(
    connection.jetstream().clone(), &index, event_store_config, messaging.topology(),
).await?;
tokio::spawn(projector.run_until_shutdown(shutdown));
let reader = NatsCorrelationIndexReader::connect(connection.jetstream().clone(), &index).await?;
let chain = reader.chain(&CorrelationId::new("R1")?).await?;
let freshness = reader.freshness().await?;
```

## What is indexed

| Source | Reference fields |
| --- | --- |
| Domain events of the context | `context`, `aggregate_type`, `aggregate_id`, `stream_version`, `event_id`, `event_type`, `causation_id`, `occurred_at` (store time) |
| Integration events published by the context | `context`, `subject`, `message_id`, `event_type`, `schema_version`, `causation_id`, `occurred_at` (envelope) |

Only headers and envelope fields are read. **Payloads are never decoded**, so
renaming or re-versioning an event payload does not affect the index. Events
without a correlation, such as fixture appends that carry none, are skipped.

**Queries are not indexed**: they append nothing. If a query result must be
traceable, carry it in the payload of the command that acts on it. Rejected
commands append nothing and are not indexed either.

Integration streams expire messages; the event store does not. A reference to an
expired integration message stays in the index and is reported with
`available: false`.

## Bucket and limits

```text
Normal: BIKE_RENTAL__CORRELATION_INDEX_READ_MODEL
Test:   BIKE_RENTAL__TEST__CORRELATION_INDEX_READ_MODEL
Keys:   correlation.<id>[/<page>], correlation-digest.<sha256>[/<page>],
        source.<domain|integration>.<context>, sources
```

One bucket serves all bounded contexts of an application. Correlation IDs that
are not valid KV keys use the digest form. A correlation keeps at most 2048
references in pages of 64; further references set `truncated: true` and are
logged. The bucket has no TTL and rejects writes when full (`DiscardPolicy::New`).

### Capacity

The default bucket capacity is 1 GiB. The index grows with every event store of
the application. A full bucket stops the projector with `CapacityExhausted` and
the host reports the worker failure; no entry is evicted. Raise the capacity
with an explicit policy update, then restart:

```rust,ignore
let index = NatsReadModelConfig::correlation_index(&application, TrafficScope::Normal)?
    .with_storage_limits(4 * 1024 * 1024 * 1024, 256 * 1024)?;
update_read_model(connection.jetstream(), &index).await?;
```

Keep the same value limit in the runtime configuration; startup verifies it.

### Rebuild

The projector stores its source checkpoints in the bucket. To rebuild:

1. Stop every runtime that projects into the bucket.
2. Delete the bucket stream (`KV_<bucket>`) with an operator credential.
3. Provision it again (`provision` or `provision_read_model`).
4. Start the runtimes. Each source replays from its first retained message.

Integration messages that already expired cannot be indexed again; their
references are lost by a rebuild. A recreated event store stream stops the
projector with `ConfigurationMismatch` until the index is rebuilt. An isolated
Test reset deletes and recreates the Test index with the other Test streams.

### Permissions

Projector credentials need the [read-model KV permissions](read-models.md#resource-policy-and-permissions)
for the bucket, plus named ephemeral consumer create, pull and delete on the
bounded context's event store stream and the application's integration-event
stream. Readers additionally need `STREAM.INFO`, `STREAM.MSG.GET`,
`CONSUMER.NAMES` and ephemeral consumer create/delete on those streams to
compute freshness and availability.

## Freshness

`freshness()` and every index view report the state of each registered source
and a per-context `lag`, computed by the server against the source streams:

```json
{
  "lag": { "billing": null, "ordering": 0 },
  "sources": [
    { "context": "billing", "kind": "domain", "stream": "SHOP__BILLING_DOMAIN_EVENTS",
      "state": "inactive", "behind": 2 },
    { "context": "billing", "kind": "integration", "stream": "SHOP_INTEGRATION_EVENTS",
      "state": "inactive", "behind": 0 },
    { "context": "ordering", "kind": "domain", "stream": "SHOP__ORDERING_DOMAIN_EVENTS",
      "state": "active", "behind": 0 }
  ]
}
```

- `behind` counts the source's matching messages after its checkpoint.
- `state` is `active` while a projector consumer for that source exists on its
  stream, `inactive` otherwise.
- `lag[context]` sums `behind` over the context's sources. It is `null` when any
  source of the context is inactive: nothing will index the missing events until
  a projector starts, and a stopped projector must not read as "no events".

`0` everywhere means the index was complete when the request was answered. It
never means that a chain has finished: later events of the same correlation can
still be written. A context appears after its projector first started.

## Tracer views

Both views require the read-only **production inspection** capability; Control
and Dispatch receive `403`. Payloads pass the configured `TracePayloadPolicy`.
The production catalog lists anchor types under `correlations.anchors`.

Declare an anchor on its aggregate definition:

```rust,ignore
impl AggregateDefinition for RentalFleetAggregate {
    type Context = BikeRental;
    type Root = RentalFleet;
    type Event = RentalFleetEvent;
    const ANCHOR: bool = true;
}
```

`GET /aggregates/{aggregate_type}/{aggregate_id}/correlations` loads the anchor
stream, collects its correlations in first-seen order and expands each from the
index. Percent-encode the aggregate type, for example
`/aggregates/bike-rental%2Frental-fleet/city-fleet/correlations`. Anchor events
include their payloads. Other streams are not loaded unless requested with
`?include=context:<name>` (repeatable, at most 16 contexts and 64 streams), which
replays each touched stream of that context. A chain that touched two anchors
appears under both.

```json
{
  "anchor": { "aggregate_type": "bike-rental/rental-fleet", "aggregate_id": "city-fleet" },
  "lag": { "bike-rental": 0 },
  "sources": [ { "context": "bike-rental", "kind": "domain", "stream": "…",
                 "state": "active", "behind": 0 }, … ],
  "correlations": [
    { "correlation_id": "R1", "truncated": false,
      "refs": [
        { "kind": "domain", "context": "bike-rental", "aggregate_type": "bike-rental/rental-fleet",
          "aggregate_id": "city-fleet", "stream_version": 3, "event_id": "event:…",
          "event_type": "bicycle-rented", "occurred_at": 1761400000000, "payload": { } },
        { "kind": "integration", "context": "bike-rental",
          "subject": "bike-rental.integration.bike-rental.bicycle-rental-started",
          "message_id": "…", "event_type": "bicycle-rental-started", "schema_version": 1,
          "causation_id": "event:…", "occurred_at": 1761400000003, "available": true }
      ] }
  ]
}
```

References are ordered by `occurred_at`; clients build the causation tree from
`event_id`, `message_id` and `causation_id`. At most 256 correlations are
returned, the most recently started; `"truncated": true` at the top level marks
omitted older ones. A non-anchor type returns `404 unknown-anchor`; a context
without configured history returns `501 history-unavailable`.

The ID must be the anchor's aggregate ID, the identity of its event stream, not
the ID of an entity inside it or of a related object. An ID without a stored
event returns `404 unknown-anchor-stream`. A stream whose events carry no
correlation, for example events appended directly by a fixture or an import,
returns `200` with an empty `correlations` list; commands executed by Rostfrei
always record one.

`GET /correlations/{id}/events` with the inspection capability returns
`{ correlation_id, truncated, lag, sources, refs }` from the index, without
payloads. It returns `404 not-found` only when the index has no reference and
is complete: at least one source is registered, every source is `active` and
none is behind. Otherwise an unknown correlation returns `200` with empty
`refs`, and `lag`/`sources` say why the answer may still change. With the
Control or Dispatch capability, the route keeps streaming Tracer-submitted
operations from memory over SSE.

Register the history of other bounded contexts with
`TracerConfig::with_production_history` (or `TracerBuilder::with_production_history`)
to anchor their aggregates and include their payloads.
