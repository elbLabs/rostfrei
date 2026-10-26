# ADR 0045: Durable correlation index and anchored correlation views

## Status

Accepted. Extends ADRs 0005, 0011, 0014, 0041 and 0044. Implements issue #123.

## Context

Every command, domain event and integration event carries a `correlation_id`
for its chain and a `causation_id` for its direct cause. The NATS event store
persists both on each `RecordedEvent` and repeats the correlation as a message
header. Tracer's `CorrelationHub` is the only correlation lookup. It is bounded
memory, only knows operations submitted through Tracer, and is cleared by every
process restart. Continuous observation (#94) sees external traffic, but only
within a 128-flow window and without backfill.

NATS filters by subject, and an event store subject must remain the aggregate
identity for `Nats-Expected-Last-Subject-Sequence` optimistic locking (ADRs
0003, 0005). "Every event of correlation R1 across all bounded contexts" is
therefore a scan of every context stream. There is also no view above one
correlation: a business object that several chains touch over time.

## Decision

- Rostfrei owns one application-scoped read model, the **correlation index**:
  `correlation_id → [message references]`. It uses the KV storage of ADR 0041.
  `NatsReadModelConfig::application_scoped(application, traffic_scope, name)`
  derives `<APP>[__TEST]__<NAME>_READ_MODEL`: the bounded-context segment of the
  existing convention is omitted and the traffic scope is kept. The index bucket
  is `<APP>[__TEST]__CORRELATION_INDEX_READ_MODEL`; all bounded contexts of an
  application write into it, Normal and Test stay apart.
- Entries contain **metadata only**. The projector reads headers and envelope
  fields; it never decodes a payload and depends on no application codec. A
  domain reference holds context, aggregate type and ID, stream version, event
  ID and type, causation and the store time of the event. An integration
  reference holds producing context, subject, message ID, event type, schema
  version, causation, `occurred_at` and its stream sequence.
- **Integration events are indexed** as their own reference kind. Causation only
  points backwards, and the first command in a consuming context is caused by
  the integration message ID; without these references the causation tree
  breaks at every context boundary. Integration streams have bounded retention
  (ADR 0014) while the event store has none. A reference to an expired message
  is kept and reported as `available: false`.
- One projector per bounded-context runtime, hosted as a supervised worker of the
  application host (ADR 0014 `NatsApplication`/`NatsRuntime`) and enabled with
  `with_correlation_index()`. It consumes that context's aggregate subjects and
  the integration subjects the context publishes. Each source keeps its stream,
  filter and checkpoint inside the index bucket; the bucket
  also lists its sources. Readers discover contexts from the bucket and need no
  configuration when a context is added. Concurrent appends to one key are
  resolved by bounded CAS retries; duplicate deliveries resolve to the same
  reference identity.
- **No index-specific retention.** The bucket has no TTL and shares the event
  store's lifetime. It keeps `DiscardPolicy::New`: a full bucket rejects writes,
  the projector stops with `CapacityExhausted`, and nothing is evicted. The
  default capacity is 1 GiB instead of the 64 MiB read-model default; operators
  raise it with `update_read_model`. `DiscardPolicy::Old` would reintroduce
  silent expiry and is not an option.
- **Bounded values.** A correlation holds at most `MAX_CORRELATION_REFS` (2048)
  references in pages of 64, keyed `correlation.<id>` and `<key>/<page>`.
  Correlation IDs outside the key alphabet use `correlation-digest.<sha256>`.
  Further references set `truncated`, are logged, and never stall the projector.
- **Rebuild** is deleting the bucket, provisioning it again and restarting the
  projector. Checkpoints live in the bucket, so every source replays from its
  first message. A recreated source stream stops the projector and requires
  the same rebuild (see the addendum on recreation).
- **Freshness is computed by the server.** The reader and every index view
  return `lag: { context → events_behind }`: the source's matching messages after its
  checkpoint, counted by JetStream. Domain and integration lag of one context are
  summed. `0` everywhere means complete as of the request, never that a chain has
  finished. See the addendum for source liveness.
- **Anchor** is a role on the aggregate definition (ADR 0020),
  `const ANCHOR: bool = true` on `AggregateDefinition`, projected as
  `"anchor": true` into the compiled domain model (ADR 0013). It is not a new
  noun: the anchor is the aggregate a view is taken from; the view is the
  anchor's correlations. This is the object-centric event log viewpoint (OCEL):
  a chain touching two anchors appears under both.
- Tracer (ADR 0011) serves `GET /aggregates/{type}/{id}/correlations`: hop 1
  loads the anchor stream through `EventHistory` and collects its correlations in
  first-seen order, hop 2 expands each from the index alone. Anchor payloads come
  from hop 1. Other streams are loaded only for `?include=context:<name>`.
  `GET /correlations/{id}/events` answers from the index for the read-only
  production inspection capability; the in-memory hub keeps serving live SSE of
  Tracer-submitted operations to Control and Dispatch. Both index views require
  `ProductionInspection` and apply `TracePayloadPolicy`.
- Queries (ADRs 0019, 0031) append nothing and are not indexed. Completed
  rejections (ADR 0044) append no events and are not indexed either.

## Consequences

Correlations of production traffic survive restarts and are readable without
Tracer having submitted them. A context added to an application appears in the
index and in `lag` after its projector first runs. Readers never compare stream
positions themselves. Index size grows linearly with the event stores; capacity
is an operator decision, exhaustion is loud.

The projector reads its sources through ephemeral ordered consumers positioned
from the checkpoint in the bucket, not through operator-provisioned durables as
the issue proposed. A durable keeps its acknowledged floor when the bucket is
deleted, so deleting the bucket alone could not rebuild the index. Operators
provision only the bucket; credentials need ephemeral consumer create, pull and
delete on the event store and integration streams, as for history replay (ADR
0042). Lag is counted with one short-lived ephemeral consumer per lagging
source. Running two projectors for one context is safe but duplicates work.

The value schema is carried by the read-model codec envelope
(`schema_version: 1`), not by a separate `schema` field in the entry.
References carry the store time of a domain event and the envelope
`occurred_at` of an integration event, as Unix milliseconds. Integration
references also store their stream sequence, used to check retention; the HTTP
views do not expose it.

An anchor view returns at most 256 correlations, the most recently started, and
loads at most 64 included streams. An anchor ID without stored events is
`404 unknown-anchor-stream`, distinct from `unknown-anchor` for a type that is
not an anchor: a caller that passes an entity ID instead of the aggregate ID gets
an error, not an empty view. An empty list remains for a stream whose events
carry no correlation, which only direct appends such as fixtures produce; the
executor always records one. A Normal bounded context named `test` with a
read model named `correlation-index` would alias the Test index bucket; do not
use that combination.

### Addendum: freshness and source liveness

A bare lag of `0` could not distinguish a caught-up source from one whose
projector had stopped: nobody advanced the checkpoint, and the server counted
nothing new against an empty stream. Readers therefore return `freshness`: per
source its context, kind, stream, `state` (`active`/`inactive`) and `behind`,
plus `lag: { context → events_behind | null }`. A context's lag is `null` when
any of its sources is inactive. Liveness is the existence of the projector's
named ordered consumer, `correlation-index--<kind>--<context>--<uuid v7>`, on
the source stream; the projector deletes it on shutdown, failure and abort, and
the server's 30-second inactivity threshold covers a lost process. No heartbeat
key is written to the bucket. `GET /correlations/{id}/events` returns `404`
only when the chain is empty and every registered source is active with
nothing behind; with no registered source it returns `200`.

Source streams must be safe to read with unacknowledged ordered consumers.
`connect` rejects with `ConfigurationMismatch` a source stream with work-queue
or interest retention, whose messages those deliveries would remove, and one
with `max_consumers > 0`. A context without integration events uses
`connect_domain_only`; it refuses a context whose integration source is already
registered, because that source would stay inactive and keep its lag `null`
until the index is rebuilt.

### Addendum: recreation is detected by sequence

The first implementation stored the source stream's `created` time as its
generation and treated any change as a recreation. JetStream does not keep that
time stable: after a server restart, a stream whose configuration had been
updated (Rostfrei updates the event store config on every boot) reported a new
`created` value with all messages and sequences intact. Every freshness read
then failed with `ConfigurationMismatch` and the Tracer views answered `503`
until a needless rebuild.

A source is now classified from its checkpoint and the stream state:
`last_sequence < checkpoint` is a recreation, since sequences never regress;
`first_sequence > checkpoint + 1` with a checkpoint above zero is a gap, since a
Limits stream may remove messages by age, size or purge while no projector runs.
A recreation still stops the projector and asks for a rebuild. A gap is
recorded as `skipped` on the source, the checkpoint moves to the first retained
sequence and projection continues. Records written with a `generation` field
are still read; the field is ignored and dropped on the next write, so an
existing bucket needs no migration. Recreating a stream with at least as many
messages as the checkpoint is not detected; deleting a source stream therefore
remains an operator step that includes the index rebuild.

Not part of this decision, optional and later: `EventHistory::load_event` for
per-event foreign payloads (next), an explicit framework event linking a chain
to an anchor it never writes to, and rejected commands as a third reference kind
fed from the outcome authority.

See [correlation index and anchored views](../correlation-index.md) for setup,
operations and the HTTP contract.
