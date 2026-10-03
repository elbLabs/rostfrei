# ADR 0041: Typed KV storage for application-owned read models

## Status

Accepted.

## Context

Applications repeatedly implement JSON/schema codecs, NATS KV revision handling,
scoped provisioning, and error translation for latest-state read models. Their
materialization rules differ: some are projections of committed domain events;
others combine application and externally authoritative facts. Issue #87 asks
for reusable storage without transferring ownership of these rules to Rostfrei.

## Decision

- Put the transport-neutral `ReadModelStore<T>`, validated keys, opaque revisions,
  explicit error kinds, and extensible versioned codec in `rostfrei-core`, exported
  through the `rostfrei` facade. NATS types stay at the application edge.
- Provide `NatsReadModelStore<T, C>` using the existing managed JetStream context.
  Reads are leader-served. Create/update/delete are per-key CAS operations;
  deletion writes a tombstone and recreation uses create-if-absent.
- Derive bucket identity from application, bounded context, model name, and
  Normal/Test traffic scope. Startup verifies policy, provisioning creates or
  verifies, and operator-owned updates are explicit.
- Applications own snapshots, source positions, duplicate/gap policies, schema
  migrations, freshness, and read-model generation cutover. Several domain and
  integration event handlers may maintain one model using the same typed store.
- Keep the authoritative EventStore and external systems authoritative. KV
  revisions are not stream versions or projection checkpoints. Model value and
  per-source progress should be persisted together before acknowledging successful
  materialization. Integration quarantine/retry exhaustion can terminally
  acknowledge a delivery without materialization; applications must reconcile
  from the authority or explicitly replay the retained quarantine record.
- Defer enumeration and watch/resume until their consistency and lifecycle
  contracts are designed.

## Consequences

Applications get reusable persistence mechanics without a new handler framework
or implicit business rules. CAS prevents lost writes on a single key; retries
and multi-key workflows still require application contracts. A failed write can
have committed, so persisted source positions are essential for safe redelivery.
TTL is retention rather than freshness. Bucket recreation is an explicit lifecycle
boundary requiring new handles and revision tokens.

See [the guide](../read-models.md) for operational guarantees, permissions,
migration, a runnable multi-handler/query example, and rebuild behavior.
