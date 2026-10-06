# Worker concurrency and shared delivery capacity

`ConsumerConfig::concurrency()` limits handlers in **one worker's `run` invocation**.
With one worker for a durable in each pod, `16` means up to 16 handlers per pod.
Multiple workers in a process have separate local limits.

`with_shared_pending_limit(...)` independently sets the unacknowledged-message budget
for **all workers sharing that durable consumer**. For example:

```rust
use std::time::Duration;
use rostfrei_messaging_core::{ApplicationName, ConsumerConfig};

let context = ApplicationName::new("shop")?.bounded_context("orders")?;
let config = ConsumerConfig::new(
    context.consumer_name("commands", 1)?,
    context.durable_name("commands", 1)?,
    context.command_address("place")?,
    Duration::from_secs(45), // Acknowledgement wait.
    Duration::from_secs(30), // Handler timeout.
    16,                     // Concurrent handlers per worker.
    5,                      // Maximum delivery attempts.
)?
.with_shared_pending_limit(128)?;
```

Use that configuration with `provision_durable_consumer` in the operator path and
with `NatsConsumerFactory::create` in each worker. Every worker on the durable must
agree on its shared pending limit and other broker policy. Local handler concurrency
can differ: workers with local limits 8 and 16 can share the same 128-message budget
without changing the durable configuration.

## Sizing

| Setting | Scope | NATS behavior |
| --- | --- | --- |
| `concurrency` | One worker | Bounds active handlers and that worker's pull batch size; valid range 1–1024 |
| `shared_pending_limit` | One durable across the fleet | Sets `max_ack_pending`; must be at least the local concurrency |

The durable's allowed maximum pull batch is `min(shared_pending_limit, 1024)`,
independent of any worker's local setting. Workers request only their local batch
size. Unsupported broker limits or values beyond NATS's signed integer range are
rejected rather than truncated.

The shared budget counts received-but-unacknowledged messages, including client
prefetch. Start with the intended fleet concurrency and allow buffering headroom,
then measure actual handler utilization and pending acknowledgements. A budget of
128 is a shared ceiling: it does not reserve 16 messages for each of eight pods or
guarantee even distribution. Increasing it does not raise a pod's handler limit.

Retain the same durable identity across replicas so they share work. Creating a
durable for each pod changes delivery semantics. Acknowledgement timeouts and
redelivery still apply; local concurrency is not a distributed execution lock.

## Existing deployments and policy changes

`ConsumerConfig::new` initially sets the shared pending limit equal to concurrency,
preserving the existing broker policy. Multi-worker capacity is an explicit opt-in
through `with_shared_pending_limit`.

To change an existing durable's shared budget, coordinate its workers and operator:

1. Deploy code supporting the separate shared limit.
2. Quiesce the affected workers and update their configuration with the agreed limit.
3. Call `provision_durable_consumer` explicitly with the new configuration, retaining
   the original durable name and delivery progress.
4. Start workers using the matching shared policy.

Workers verify broker configuration at startup and do not resize it themselves.
Changing only local concurrency with a fixed explicit shared limit requires no
durable-policy update, provided the local limit remains within the shared budget.
Ordered domain-event/read-model workers retain their separate delivery contracts.

See [consumer concurrency tests](../crates/rostfrei-nats/tests/consumer_concurrency.rs)
for the executable shared-capacity contract.
