# Runtime reference

## Event history and accepted commands

NATS event storage requires NATS Server 2.12.1 or newer. One event transaction
can atomically append commits to multiple aggregate streams in the same
bounded-context event store.

Accepted commands persist an acceptance receipt, including commands that emit
no domain events or load no aggregates. Event-free acceptance returns
`CommandReceipt::AcceptedNoEvents`; retries return `ExactReplay` with an empty
event list. Loaded read-only aggregates are guarded atomically with the receipt.
Changed command content or provenance conflicts under the accepted operation
identity. See [durable event-free acceptance](adr/0039-durable-event-free-acceptance.md)
for concurrency and migration from transient no-op results.

Aggregate histories use native ordered, exact-subject ephemeral replay without advancing
application durable consumers. Reader credentials need scoped replay-consumer
create/pull/delete and inbox permissions, plus stream-info/raw-message access.
The native iterator uses 500-message pulls without a batch byte cap; each
next-message wait, including recovery, has a five-second deadline.

By default, history loads trust committed writes and validate local envelopes,
checksums, identities, ordering, and commit structure. `audit_history` and
`audit_streams` add historical receipt, participant, and guard checks. Use
`store.with_history_auditing(true)` to enable these checks on every load, or
`false` for trusted reads. The setting also applies to session loads and
discovery; local and operation-specific write/retry checks always run. See
[the read/audit trust boundary](adr/0040-trusted-event-store-reads-and-explicit-audits.md).

## NATS authentication

`NatsConnectionConfig` supports connection-wide username/password authentication,
including reconnects and failover:

```rust
use rostfrei_nats::{NatsConnectionConfig, connect};

let config = NatsConnectionConfig::from_server_pool(
    "my-application",
    ["nats://nats-a:4222", "nats://nats-b:4222"],
)
.with_user_and_password(
    std::env::var("NATS_USERNAME")?,
    std::env::var("NATS_PASSWORD")?,
);
let connection = connect(&config).await?;
```

Credentials in URLs, such as `nats://app:p%40ss%3Aword@nats-a:4222`, are
percent-decoded once. Explicit builder values are literal. Both username and
password must be nonempty. A URL-only pool must contain the same complete pair
on every entry, or no credentials on any entry. With explicit credentials, URLs
may omit the pair; embedded credentials must be complete and match. Conflicting
configuration is rejected before connecting.

Token and callback authentication remain available through the managed
connection API. They cannot be combined with username/password credentials in
server URLs: select one connection-wide method.

Credentials are removed from server addresses before they reach the NATS client
and omitted from configuration debug output and returned errors. The resolved
credentials also apply to servers discovered by NATS.

The pinned `async-nats` 0.50.0 dependency logs raw CONNECT fields, including
credentials, at TRACE level. Keep `async_nats::connection` logging below TRACE
when using authentication; Rostfrei's configuration redaction does not filter
the dependency's protocol logs.

## Command execution metadata

`CommandExecutor::execute` and `CommandBus` share the correlation rule
`correlation_id = supplied_correlation_id ?? operation_id`. Direct execution
establishes the default before invoking a handler, checking an existing receipt,
or persisting events. Committed events and their receipt carry the same
correlation. An exact retry retains it; a follow-up operation can supply the
earlier correlation to continue the flow.

Supplied causation is preserved independently. Direct execution leaves omitted
causation absent; the bus processor uses the command message ID when none was
supplied. `CommandExecutor::simulate` applies the same correlation default to
handler metadata. An operation ID used as the default must satisfy
`CorrelationId` validation; an invalid default fails before execution.

## Event delivery and read models

The [domain-event worker pool](design/domain-event-worker-pool.md) lets overlapping
application instances share a durable with explicit ACKs, ordered transaction
processing, and graceful shutdown. Existing `AckPolicy::All` durables need the
documented one-time migration.

[Event-driven read models](read-model-runtime.md) use typed event transformations
to maintain saved query views. The runtime loads, checkpoints, retries CAS
conflicts, and persists values. Integration inputs default to broker order,
with optional business-version policies. The underlying
[typed storage API](read-models.md) provides revision-aware reads, CAS writes,
versioned codecs, and explicitly provisioned Normal/Test-scoped buckets.

[Continuous observation](continuous-observation.md) and
[quarantine inspection](quarantine-inspection.md) expose application evidence
through Tracer. Production observation uses the read-only inspection capability.
