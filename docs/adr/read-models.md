# Read models

**A view answers a question; it does not replace its source of truth.**

Saved query state can make reads cheap, but it may lag, omit facts, or outlive
the source records it references. Its convenience must not imply authority or
completeness.

## What follows

- Keep event history and external source systems authoritative. The owner of a
  view defines its meaning, freshness, schema, rebuild, and generation cutover.
  The same obligation applies to application and framework-owned views.
- Provide transport-neutral typed storage, opaque revisions, and per-key CAS;
  use scoped NATS KV buckets as the adapter. Queries receive read-only handles.
- Persist each transformed value and its source progress together. Keep storage
  revisions, source positions, and aggregate versions distinct.
- Let the runtime own retry and catch-up mechanics. Transformations must be
  repeatable; source ordering and business-version ordering are different contracts.
- Make retention and capacity policy explicit. A rebuild can recover only what
  its sources still retain; a saved reference cannot recreate an expired payload.
  Lag, source availability, and truncation must not be confused with an empty result.

## Tradeoff

Materialization buys predictable query cost at the price of storage, processing,
and possible lag. CAS protects one key, not a multi-key transaction. Recovery
must respect source retention rather than promise that every view is rebuildable.

See [event-driven read models](../read-models/index.mdx) and
[typed storage](../read-models.md).
