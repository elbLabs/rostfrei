# Domain-event workers

Replicas share one durable pull consumer per logical subscriber. Different
subscribers retain independent progress. This is ordered work sharing: NATS picks
the worker for each delivery, while one transaction is processed at a time.

The durable uses explicit ACKs, `MaxWaiting = 512`, `MaxAckPending = 1`,
`MaxRequestBatch = 1`, and unlimited redelivery with configured `AckWait` and retry
delay. Source history uses the authoritative journal's Limits retention.

The domain filter is `<domain-prefix>.aggregate.*.*.*.*`: aggregate type, aggregate
ID, correlation, and causation occupy separate tokens. It excludes receipts and
read guards. Consumers validate the subject against the canonical event envelope;
see [subjects and filters](messaging/subjects.mdx#filters-and-internal-storage-subjects).

## Transaction delivery

1. Receive the next source event and inspect confirmed durable progress.
2. Load and validate the complete committed transaction from history.
3. Invoke handlers in transaction order; integration publication awaits PubAck.
4. Confirm the first delivery only after every handler succeeds.
5. Confirm remaining transaction events individually after verifying membership
   against the immutable first event. Its persisted ACK proves processing finished.

A crash before the first ACK retries the whole transaction and may repeat effects.
A crash afterward lets another worker confirm the tail without republishing it.
The single-pending limit and ACK-after-whole-transaction rule are essential; manual
ACKs or changing that limit invalidate this contract. Permanent/corrupt work blocks
progress rather than being skipped.

Integration IDs derive from source event, logical address, and schema. The physical
correlation/causation suffix does not change these identities or durable names.
The publisher derives that suffix from a validated integration envelope and rejects
inconsistent transport metadata. Broker deduplication is bounded (default two
minutes); downstream integration-command mappings retain stable operation IDs.
Arbitrary external effects still require idempotency.

## Shutdown

Stop new pulls, let an outstanding finite pull finish, and NAK late deliveries
without dispatch. Let started transactions finish within `processing_timeout`,
confirm successful work, then stop. After failed work stops, shutdown NAKs it
immediately; normal operation uses the retry delay. Crashes fall back to `AckWait`.

Allow pod termination grace for processing, ACK confirmation, and connection drain.
Late old attempts can overlap redelivery; shutdown is not an execution fence.

## Existing durable migration

This ACK-policy procedure assumes compatible source subjects. The current
event-subject format requires [fresh namespaces](messaging/subjects.mdx#fresh-deployments);
recreating a durable does not convert old history or its filters.

NATS cannot update legacy `AckPolicy::All`/one-waiter consumers in place. Startup
rejects them rather than silently recreating them. For each affected durable:

1. Stop all old workers and wait for processing to end.
2. Save configuration and confirmed `ack_floor.stream_sequence`, not delivered
   progress. Keep the source stream.
3. Delete only the stopped durable and recreate it with the new policy:

   ```rust,ignore
   let next = saved_ack_floor.checked_add(1).ok_or("sequence overflow")?;
   let config = config.with_start_sequence(next)?;
   provision_domain_event_consumer(&jetstream, &event_store, &config).await?;
   ```

4. Retain that start sequence in subsequent worker/provisioning configuration.
   It selects initial creation policy and does not rewind an existing durable.
5. Start new workers and verify progress. If recreation fails, retry with the saved
   sequence; default `DeliverPolicy::All` would replay history. Rollback requires
   a compatible reader or a separately planned replay.

Subsequent compatible deployments can overlap normally. For the executable
contract, see [consumer integration tests](../crates/rostfrei-nats/tests/domain_event_consumer_integration.rs).
