# KV-backed read models

For the high-level `#[derive(ReadModel)]` and event-handler builder, start with
[event-driven read models](read-model-runtime.md). That runtime handles loading,
checkpointing, CAS retries, and persistence. This page documents the underlying
storage contract and the lower-level, application-owned orchestration example.

For measured query costs, see [KV versus two-aggregate replay](read-model-benchmark.md):
a reproducible real-NATS benchmark with 100 total events and 100 events per aggregate,
parallel replay, and an already-loaded-history baseline.

`ReadModelStore<T>` (exported by `rostfrei` and `rostfrei-core`) stores an
application-owned, key-addressed snapshot. `NatsReadModelStore<T, C>` implements
it using NATS KV. The application supplies the schema, codec, materialization
rules, source checkpoints, and freshness/authorization policy.

**One read model can have multiple domain-event and integration-event handlers.**
Give them the same `Arc<dyn ReadModelStore<Snapshot>>`; CAS protects concurrent
updates to one key. A projection derives its state from committed domain events.
A model incorporating integration events or external facts can use the same
storage contract, with each source retaining its own authority.

## Setup

```rust
use std::{num::NonZeroU32, sync::Arc};
use rostfrei::{ApplicationName, JsonReadModelCodec, ReadModelStore};
use rostfrei_nats::{NatsReadModelConfig, NatsReadModelStore, provision_read_model};

#[derive(serde::Serialize, serde::Deserialize)]
struct Entitlement {
    members: u64,
    paid: bool,
    organization_version: u64,
    billing_version: u64,
}

let context = ApplicationName::new("fast-inbox")?
    .bounded_context("commercial-access")?;
let policy = NatsReadModelConfig::new(&context, "organization-entitlement")?
    .with_storage_limits(64 * 1024 * 1024, 256 * 1024)?;

// Explicit operator/bootstrap operation, separate from normal startup.
provision_read_model(connection.jetstream(), &policy).await?;

let store: Arc<dyn ReadModelStore<Entitlement>> = Arc::new(
    NatsReadModelStore::connect(
        connection.jetstream().clone(),
        policy,
        JsonReadModelCodec::new(NonZeroU32::MIN),
    ).await?,
);
```

The adapter clones the existing managed JetStream context, sharing its connection,
credentials, request timeouts, and reconnect policy. It creates no connections,
subscriptions, consumers, or background tasks. Dropping a handle does not delete
the bucket; the application owns the managed connection's shutdown.

## Storage contract

| Operation | Result |
| --- | --- |
| `read(&key)` | `Some(ReadModelEntry { value, revision })`, or `None` for absent/deleted/expired data |
| `create(&key, &value)` | Committed revision; conflicts if a live value exists |
| `update(&key, &revision, &value)` | Committed revision; conflicts if the expected revision is stale or the key is missing |
| `delete(&key, &revision)` | CAS-protected tombstone; repeating with the old token conflicts |

`ReadModelKey::new` validates concrete, bounded ASCII keys; wildcards and empty
dot segments are rejected. Keys are limited to 256 bytes. See its API docs for
the accepted alphabet. Encode application identities deliberately rather than
silently replacing unsupported characters, which can create collisions.

Revision tokens are opaque and bound to a bucket incarnation and key. Retain
them, but do not increment them or compare their numerical order. Successful
writes wait for a JetStream persistence acknowledgement. The provisioned bucket
disables direct replica reads, so the adapter uses leader-served reads. CAS is
atomic for one key; it is not a cross-key transaction or a guarantee that a query
has caught up with its event sources. Durability follows the configured NATS
storage/replica policy; memory storage does not survive a full broker restart.

Deletion retains a KV tombstone according to history/TTL policy. A read treats it
as missing. `create` recreates it by CAS against that tombstone, giving the new
value a new revision; stale live revisions cannot resurrect or delete it. TTL
expiry is also missing and requires `create`. Recreating an entire bucket is an
operator lifecycle operation: stop its users, discard old handles/tokens, then
reconnect. A fresh handle rejects tokens from the previous incarnation.

### Errors and retries

`ReadModelError::kind()` distinguishes:

- `InvalidRequest`: invalid key/policy or a token for a different key/bucket.
- `Conflict`: competing create or stale update/delete. Reload, re-check source
  progress, recompute, and retry with a bounded contention budget/backoff.
- `Unavailable`: broker/request/authentication failures or missing infrastructure.
  **A failed or timed-out write may still commit**, including after reconnect.
  Reload and use your persisted source checkpoint to resolve ambiguity.
- `InvalidData`: malformed JSON/envelope/application value.
- `IncompatibleSchema`: a codec deliberately rejects the stored version.
- `EncodingFailed`: the supplied value cannot be serialized.
- `PayloadTooLarge`: encoded value, bucket wire-message, or server payload limit.
- `CapacityExhausted`: bucket/account storage limits reject a write.
- `ConfigurationMismatch`: an existing bucket has incompatible policy/routing.

Missing data is `Ok(None)`, never confused with unavailability. Application codecs
may add business validation while retaining these error categories. Error messages
from the built-in codec do not include stored payloads.

`JsonReadModelCodec<T>` writes `{"schema_version":1,"value":{...}}` and accepts
exactly its configured nonzero schema version. A custom `ReadModelCodec<T>` can
decode/upcast supported old versions while encoding only the current one. Deploy
read compatibility before new writers; use a separately named model generation
when old readers cannot interpret new values.

## Multiple handlers and source ordering

The runnable [storage example](../crates/rostfrei-nats/examples/read_model_storage/main.rs) and
[application code](../crates/rostfrei-nats/examples/read_model_storage/application.rs)
include:

- Two `DomainEventHandler` implementations (`MemberJoined`, `DemoChanged`),
  registered on one `DomainEventDispatcher` and sharing one read model.
- A `MessageHandler<IntegrationEventAddress>` for public billing facts, attached
  to its own integration-event consumer in a service.
- A typed `QueryHandler<String, Option<Entitlement>>` reading through the
  application-owned `EntitlementLookup` port.
- A `rebuild` function loading committed events via `EventHistory` and dispatching
  through the same domain handlers. The runnable example uses `NatsEventStore`.

Run it against an isolated broker:

```sh
python3 scripts/test_nats.py -- cargo run --locked -p rostfrei-nats --example read_model_storage
```

Each handler reads the entire snapshot, checks its own source position, changes
only its fields, and writes **value plus source position in one CAS**. On conflict,
it starts again from a fresh read. Successful materialization returns ACK only
after persistence (or after confirming that the source position is already
applied). A crash between persistence and ACK can cause redelivery; the stored
position makes that duplicate a no-op. Any application-maintained materialization
checkpoint must follow the same ordering.

Consumer progress is a delivery outcome, not proof of materialization. Domain
storage/configuration failures use the existing retryable/operator-blocking
domain-consumer contract; a source gap blocks progress until history is repaired.
The integration consumer has a different terminal path: an explicit quarantine
disposition, or exhaustion of configured retries, persists a quarantine record
and sends a terminal ACK. Its durable can advance while the snapshot and its
`billing_version` remain unchanged. Fixing storage alone does not redeliver that
event. See [integration recovery](#integration-quarantine-and-recovery) below.

Three positions have different meanings:

1. **KV revision:** storage CAS token for this snapshot.
2. **Aggregate stream version:** committed event position within one aggregate.
3. **Projection source position:** application-owned progress for each input source.
   This example stores an organization stream version plus an independent billing
   source version. For several aggregates/contexts, namespace a checkpoint map by
   source identity; a single global “last event” field is insufficient.

Organization events in the example are ordered deltas: only the next contiguous
stream version is applicable; duplicates are skipped and gaps are blocked. Every
event in that source stream must advance progress, including intentional no-ops.
A consumer filtering out source events cannot assume contiguous aggregate versions
without loading the missing history. In contrast, the billing contract supplies a
complete replacement fact with an authoritative monotonic per-organization version,
so a newer billing fact supersedes older ones even with skipped versions. Do not
substitute delivery order, timestamps, or unordered webhook IDs for that contract.

For multiple read models consuming the **same event type**, use independently
checkpointed durable consumers/dispatchers. A dispatcher currently permits one
registered handler per aggregate/event pair; multiple event types can share one
handler object. A shared fan-out handler would need each destination to tolerate
retries after partial success.

Multi-key materialization is not atomic. A handler may persist one key and fail on
the next. Use per-key idempotency, reconciliation, or an application-owned staged
generation/cutover protocol. There is no cross-key transaction or exactly-once
delivery promise.

### Integration quarantine and recovery

The example quarantines permanent storage/configuration failures immediately.
Transient unavailability retries only up to the integration consumer's configured
attempt limit, then also enters quarantine. For example, a `paid=false` billing
fact at source version 8 can be quarantined while the snapshot still contains
`paid=true` at version 7. Consumer ACK progress cannot establish that version 8 was
materialized, and storage recovery does not automatically change that snapshot.

Applications must monitor quarantined materialization failures and implement a
recovery policy. After repairing storage/configuration, either:

1. **Republish the quarantined event.** Validate its application/context/address,
   schema, and complete payload (`payload_truncated` must be false). Publish to the
   intended integration-event subject with a new transport/envelope message ID so
   JetStream's duplicate window does not suppress the recovery delivery. Preserve
   the logical billing source version, original occurrence time, and correlation;
   link the new delivery to the original with causation and recovery metadata.
   The normal handler then applies it through the same source-checkpoint/CAS path.
2. **Refresh from the billing authority.** Fetch its latest complete fact and pass
   it through `Entitlements::billing_changed`. Its monotonic source version makes
   an already-applied or superseded quarantined event harmless. The application
   schedules retries of this refresh until it succeeds.

Retain the original quarantine record. A successful republication PubAck only
confirms enqueueing, so mark recovery complete after verifying the snapshot's
billing checkpoint has reached or superseded the failed version. Query freshness
and authorization policies still decide how to treat a lagging snapshot.

The [broker-backed recovery tests](../crates/rostfrei-nats/tests/read_model/recovery.rs)
exercise immediate capacity-failure quarantine and unavailable-storage retry
exhaustion against a real consumer and KV bucket. They verify terminal consumer
progress with an unchanged snapshot, duplicate-window suppression of the old
message ID, successful explicit republication after repair, and idempotent replay.

### Rebuild and externally sourced facts

Provision a new named generation, replay each authoritative aggregate stream in
order, catch up live changes from a defined handoff position, verify it, and switch
queries to that generation. The integration test demonstrates rebuilding into a
second bucket while the serving model remains available. A live, multi-source
cutover must coordinate its own source watermarks; a completed replay alone is not
proof that the new generation is current. Alternatively pause consumers/queries
during an in-place rebuild, and reset the consumer checkpoint to match the model.

Stripe remains authoritative for billing facts. Re-fetch those facts through an
application-owned port during reconciliation/rebuild, then CAS-update the model
using that source's freshness/version policy. Unordered webhooks can trigger a
refresh rather than being interpreted as ordered deltas. KV TTL only governs
retention. Store and check `refreshed_at`, `valid_until`, or authorization deadlines
in application data; an unexpired KV entry need not be fresh or authorize access.

## Resource policy and permissions

Names use existing scope conventions, with `-` becoming `_` and uppercase tokens:

```text
Normal: FAST_INBOX__COMMERCIAL_ACCESS__ORGANIZATION_ENTITLEMENT_READ_MODEL
Test:   FAST_INBOX__TEST__COMMERCIAL_ACCESS__ORGANIZATION_ENTITLEMENT_READ_MODEL
Stream: KV_<bucket>
Keys:   $KV.<bucket>.<key>
```

Application, bounded context, read-model name, and traffic scope each isolate a
bucket. Names are validated lowercase kebab-case scope segments. Use
`bounded_context_in_scope(TrafficScope::Test, "commercial-access")` for Test.

Defaults: 64 MiB bucket capacity, 256 KiB encoded value limit, file storage, one
replica, one history entry per key, no TTL. Builders configure capacity/value
limits, storage, 1–5 replicas, 1–64 history entries, and optional positive TTL.
The underlying stream reserves an additional 1 KiB per-message header allowance.
Bucket capacity counts broker overhead as well as values. At capacity, new writes
are rejected rather than silently evicting unrelated keys.

`connect` and `verify_read_model` only verify. `provision_read_model` creates or
verifies without changing existing policy, including under racing provisioners.
`update_read_model` explicitly updates an existing correctly scoped bucket; NATS
still enforces server restrictions (for example storage backend changes). Capacity,
history, or TTL reductions can remove data. Reconnect handles after changing value
limits; verification is startup-time, not a continuous policy controller.

For the default JetStream API prefix, runtime credentials need:

- Publish/request `$JS.API.STREAM.INFO.KV_<bucket>` for connection verification.
- Publish/request `$JS.API.STREAM.MSG.GET.KV_<bucket>` for leader-served reads.
- Writers publish `$KV.<bucket>.>` and receive persistence replies.
- Subscribe to the client's reply inbox subjects (normally `_INBOX.>`).

Provisioners additionally request `$JS.API.STREAM.CREATE.KV_<bucket>`; deliberate
policy updates need `$JS.API.STREAM.UPDATE.KV_<bucket>`. Destructive lifecycle tools
need `$JS.API.STREAM.DELETE.KV_<bucket>`. Query-only credentials can omit write and
provisioning subjects. Scope permissions separately for Normal/Test buckets and
include existing connection health/version-discovery requirements. Custom
JetStream API prefixes/domains require the corresponding API permissions. These
operations do not require consumer-create or watch permissions.

## Migrating a direct async-nats adapter

Keep your `EntitlementLookup`/repository interface and business rules. Replace its
KV decoding, revision/CAS, and error-mapping implementation with an injected
`Arc<dyn ReadModelStore<Entitlement>>`, as in the example. Then:

1. Provision the derived bucket with explicit limits; do not alias an arbitrary
   legacy bucket into another application or traffic scope.
2. Pause old writers or establish a replay/catch-up boundary. Read known entity keys
   through the old adapter, validate/upcast values, and `create` them in the new
   store. Copy source checkpoints with the values, **not legacy KV revisions**.
3. For an external-source model, prefer re-materializing from the authoritative
   source if old values lack valid source checkpoints/freshness metadata.
4. Switch the application's injected repository and consumers together, verify
   queries, and retire the legacy bucket after your rollback window.

The copy's essential operation is `new_store.create(&validated_key, &old.value)`;
handle a conflicting destination through reconciliation, not unconditional overwrite.
A custom codec can accept the old payload shape for a transitional import without
putting entitlement/business rules into Rostfrei.

## Deferred APIs

Key enumeration and watch/resume are follow-up work. This release supports bounded
point operations only and owns no watch resources. Rebuild enumerates identities
from authoritative sources (`StreamDirectory` where appropriate) or an application
directory. A future watch API needs explicit snapshot-to-watch handoff, reconnect
and history-gap detection, resumable cursors, and cancellation/consumer ownership
before it can safely support local caches.
