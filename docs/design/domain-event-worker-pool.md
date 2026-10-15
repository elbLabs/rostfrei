# Rolling domain-event workers on a shared durable

Tracking: [#104](https://github.com/elbLabs/rostfrei/issues/104).

## Implemented design

Pod replicas use one JetStream durable pull consumer per logical subscriber.
This is NATS's [worker-pool pattern](https://docs.nats.io/learn/jetstream/worker-pool).
Consumers for different logical subscribers retain independent progress.

The domain-event durable uses:

- `AckPolicy::Explicit`;
- `MaxWaiting = 512`, so overlapping pods can wait together;
- `MaxAckPending = 1` and `MaxRequestBatch = 1` to preserve ordered source progress;
- unlimited redelivery with the configured `AckWait` and retry delay;
- the existing authoritative stream with Limits retention.

This is **ordered work sharing**, not concurrent processing of unrelated
transactions on the same durable. NATS chooses the worker for each delivery.
Neither a leader lease nor pinned-client support is required. A healthy worker
waiting for work is ready; it does not need to receive an event first.

## Multi-event transactions

The old loop pulled a transaction's first event and then separately pulled its
tail from the durable. Competing workers could split those deliveries and block
each other. The new loop reads the tail from already committed stream history,
using bounded concurrent reads, instead of pulling more work from the durable.

1. Receive the next source event and inspect the durable's confirmed progress.
2. If it starts unprocessed work, load and validate the complete transaction,
   including its existing receipt when required by the stored schema.
3. Invoke all handlers in transaction order. The integration publisher waits for
   each PubAck and uses its existing deterministic message identity.
4. Only after **all** handlers succeed, explicitly confirm the first delivery.
5. The remaining source events are delivered and explicitly confirmed one at a
   time. The first event's persisted ACK proves that the whole transaction was
   processed. A worker verifies the tail's membership against the immutable
   first event before using that proof; it does not publish the transaction again.

Consequently, another pod can finish the ACK tail after the original pod exits.
A crash before the first ACK retries the complete transaction, potentially
repeating already successful effects. A crash after that ACK only requires the
remaining confirmations. No new checkpoint, receipt, or coordination event is
written. The single-pending-message constraint and ACK-after-whole-transaction
rule are essential to this interpretation of the existing ACK floor.

Source history must remain immutable and retained as required by the event-store
contract. Manual acknowledgements that bypass transaction processing violate the
consumer contract, as does changing its pending-message limit while workers run.

## Shutdown and recovery

- Stop issuing new pulls when shutdown is requested or its signal channel closes.
- Let an outstanding finite pull finish (normally at most one second). If it
  delivers after shutdown was requested, NAK that message without dispatching it.
  This avoids abandoning an inbox with an outstanding server-side pull.
- Let a started transaction finish within `processing_timeout`. This budget covers
  progress inspection, history loading, validation, and handler execution.
- Confirm successful work, then exit before starting another pull.
- After failed/timed-out work has stopped, NAK it immediately on shutdown so a
  waiting worker can take over. During normal operation, use the retry delay.
- A process crash leaves the delivery unacknowledged; another worker retries it
  after `AckWait`. Permanent/corrupt work is not acknowledged or skipped.

Set the pod termination grace period to accommodate processing, ACK confirmation,
and connection drain. Forced termination falls back to redelivery. A late old
attempt can overlap a retry, so the existing at-least-once/idempotency contract
still applies.

## Duplicate publication

Integration message IDs are derived from source event identity, integration
address, and schema version. Two workers publishing the same mapping therefore
send the same `Nats-Msg-Id`; NATS suppresses duplicate storage within the stream's
duplicate window (provisioning default: two minutes).

Outside that window, duplicate bus records are possible. Downstream Rostfrei
integration-command mappings reuse a deterministic operation ID and the existing
command replay machinery. Mappings must remain semantically consistent for the
same identity. This is not an unlimited exactly-once guarantee for arbitrary
external effects.

## Existing durable migration

Newly provisioned installations use the worker-pool configuration immediately
and need no migration. Automated Flux migration jobs are outside this change.

NATS cannot change an existing durable's ACK policy or waiting-pull limit in
place. Legacy `All`/one-waiter consumers are rejected with an explicit migration
diagnostic. Startup does not delete or silently recreate them.

For each affected durable:

1. Stop **all** old workers using it and wait for their processing to end. This
   first compatibility cutover requires quiescing those workers; subsequent
   deployments can overlap normally.
2. Save its consumer configuration and confirmed `ack_floor.stream_sequence`.
   Use confirmed progress, never `delivered.stream_sequence`.
3. Keep the source stream. Delete only the stopped legacy durable.
4. Recreate it with the new configuration and the saved confirmed sequence plus
   one as its initial position:

   ```rust,ignore
   let next = saved_ack_floor.checked_add(1).ok_or("sequence overflow")?;
   let config = config.with_start_sequence(next)?;
   provision_domain_event_consumer(&jetstream, &event_store, &config).await?;
   ```

5. Keep that `with_start_sequence` value in the worker/provisioning configuration
   on subsequent starts. It selects `DeliverPolicy::ByStartSequence` at creation;
   it does not rewind the persisted progress of an existing durable.
6. Start the new workers and confirm progress. Retain the saved configuration and
   position until the cutover is verified. Rollback requires a reader compatible
   with the resumed durable or a separately planned replay; merely starting the
   old binary against the new ACK policy is not supported.

If recreation fails after deletion, retry with the saved starting position.
Recreating with the default `DeliverPolicy::All` instead would replay history.

## Verification

The production-consumer integration suite covers:

- simultaneous waiting pulls on one durable;
- ordered multi-aggregate transaction processing across two independent clients,
  including the maximum 100-event transaction size;
- no source ACK before the whole transaction succeeds;
- graceful shutdown after only the first delivery's ACK, followed by another
  worker confirming the tail without repeating effects;
- crash recovery before the first ACK, preserving event identities and order;
- immediate handoff of failed work on shutdown despite a long `AckWait`;
- a late delivery arriving during idle shutdown;
- an actual integration publication followed by a crash before source ACK,
  with a duplicate PubAck on takeover and only one bus record per event;
- explicit migration preserving confirmed progress and rejection of legacy mode;
- existing independent-durable, receipt-validation, retry, and blocking contracts.

```sh
python3 scripts/test_nats.py -- \
  cargo test --locked -p rostfrei-nats --test domain_event_consumer_integration
```

The event-store minimum remains NATS 2.12.1. The earlier
[pinning investigation](pinned-domain-event-handoff.md) and its example are
retained as research; their pinning-specific version/client limitations do not
apply to this implementation.

Verified locally on 2026-10-09:

- All nine domain-event consumer integration tests passed on NATS 2.12.1.
- The default workspace suite (`python3 scripts/test_nats.py`, all features)
  passed on NATS 2.12.15, including the worker-pool and 100-event coverage.
  The default suite excludes the five opt-in native-server/TLS fixtures.
- Workspace Clippy (all targets/features, warnings denied), formatting, the
  workspace domain-structure check, CLI packaging, and all 21 Python runner/version
  tests passed.
