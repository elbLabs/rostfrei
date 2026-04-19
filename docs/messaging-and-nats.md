# Messaging and NATS

rostfrei scopes messaging to one application and its bounded contexts. The
application is declared once and then reused to create typed addresses and NATS
configuration.

```rust
use rostfrei_messaging_core::ApplicationName;
use rostfrei_nats::{ApplicationMessagingConfig, NatsEventStoreConfig};

let application = ApplicationName::new("fast-inbox")?;
let commercial_access = application.bounded_context("commercial-access")?;

let messaging = ApplicationMessagingConfig::new(&application)?.with_replicas(3)?;
let event_store =
    NatsEventStoreConfig::for_bounded_context(&commercial_access)?.with_replicas(3)?;

let evaluate = commercial_access.command_address("evaluate")?;
assert_eq!(
    evaluate.as_str(),
    "fast-inbox.command.commercial-access.evaluate"
);
```

The messaging configuration derives:

| JetStream stream | Subject filter | Retention |
| --- | --- | --- |
| `FAST_INBOX_COMMANDS` | `fast-inbox.command.>` | Work queue |
| `FAST_INBOX_COMMAND_RESPONSES` | `fast-inbox.command-response.>` | Limits |
| `FAST_INBOX_INTEGRATION_EVENTS` | `fast-inbox.integration.>` | Limits |
| `FAST_INBOX_QUARANTINE` | `fast-inbox.quarantine.>` | Limits |

Command, command-response, and integration-event payloads are limited to 1 MiB.
Their streams reserve an additional 64 KiB for bounded caller metadata, tracing,
and adapter headers; quarantine records remain limited to 2 MiB. If a malformed
source message cannot fit after base64 encoding, its quarantine record retains a
bounded payload prefix together with the original size and SHA-256 digest.
Existing command and integration-event streams using the former 2 MiB maximum
must be reprovisioned before upgraded publishers and consumers start.

The bounded-context event store derives:

```text
FAST_INBOX__COMMERCIAL_ACCESS_DOMAIN_EVENTS
fast-inbox.domain.commercial-access.aggregate.*
```

Individual aggregate histories use opaque subjects below that prefix. Stored
domain-event envelopes also record `fast-inbox` and `commercial-access`; replay
rejects events scoped to another application or bounded context.

## Provisioning

Provisioning is explicit and operator-owned:

```rust
use rostfrei_nats::{provision_application_messaging, provision_event_store};

provision_application_messaging(connection.jetstream(), &messaging).await?;
provision_event_store(connection.jetstream(), &event_store).await?;

// Runtime startup verifies the operator-provisioned messaging policy.
connection.verify_application_messaging(&messaging).await?;
```

Runtime services connect to and verify provisioned infrastructure. They do not
mutate streams during ordinary startup. Verification compares the complete
rostfrei-owned stream policy, including retention, capacity, replication,
delete and purge controls, rollup, sealing, transforms, persistence mode, and
supported message-lifecycle features.

Publishers, consumers, query requesters, query servers, and domain-event
consumers reject addresses or durable names belonging to another application.
Command consumers and private domain-event consumers also remain in their
configured bounded context; integration-event consumers may cross contexts
inside the same application.

## Command dispatch

The control plane keeps simulation and live publication on separate routes. A
deployment registers a command-specific `DispatchAdapter` and explicitly mounts
the dispatch router with its own bearer capability:

```text
POST /v1/contexts/{context}/aggregates/{aggregate}/{id}/commands/{command}/dispatch
```

Dispatch requires `Idempotency-Key`. The target aggregate, command contract,
schema version, operation identity, and payload are carried in a versioned
command envelope. Consumers recompute the operation fingerprint before calling
`Executor::execute`; they do not trust a producer-supplied fingerprint.
Broker deduplication IDs bind the operation identity to that fingerprint, so a
duplicate PubAck confirms an exact wire retry rather than different content
submitted under a reused operation identity.
The example NATS adapter makes bounded retries for publication timeouts and
broker unavailability with the same message identity before reporting a
terminal operation failure. After a PubAck, it reads responses in bounded
30-second slices. A slice timeout or transient reader unavailability starts
another slice; the commander keeps listening until a terminal response arrives
or its operation task is cancelled. Invalid configuration, an invalid response,
or an identity conflict remains terminal.
Adapters may declare a lower payload limit than the control-plane maximum so
envelope overhead is rejected during admission rather than after publication
work has started.

A JetStream command PubAck proves publication only. The dispatch adapter reports
it through `DispatchObserver`, so the live operation emits `command.published`
with the command message identity while the adapter continues waiting. The
command consumer executes the command, derives its exact response address, and
publishes one immutable accepted or rejected `CommandResponse`. It acknowledges
the command only after the response PubAck. Transient response publication
failures retry without acknowledging the command.

Command responses have their own v1 wire schema, independent of the originating
command schema. Each response carries the originating `CommandAddress`, and
publishers and readers derive its exact subject from that address plus the
operation and command-message identities. A mismatched subject is invalid.

Before executing any delivery, including a redelivery, the worker performs a
short exact lookup at that response address. A matching retained response means
the command was already answered, so the worker ACKs without executing the
aggregate again. Absence, reported as a lookup timeout, permits execution;
response-store unavailability retries without execution. Invalid or conflicting
stored responses are quarantined and never passed to the aggregate.

The trace emits `command.responded`, `command.accepted` or `command.rejected`,
and finally `operation.completed` with the same terminal decision. The result
carries the command and response message identities and the command PubAck
duplicate flag; it does not invent a base stream version or append evidence.
The `appended` field is omitted for dispatch because the response does not carry
authoritative persistence evidence. Simulation retains `appended: false`
because simulation is guaranteed not to append. Business rejection does not
append a domain event. Command and rejection payloads remain subject to the
configured trace redaction policy.

This is not an exactly-once terminal-outcome protocol. Accepted commands that
append events can recover through event-store exact replay, and a retained
response prevents execution after its PubAck. Rejected decisions and accepted
decisions that emit no event still have a crash window between deciding and
persisting the response: there is no transactional operation receipt or outbox,
so redelivery can evaluate them again. Response-subject immutability also lasts
only while the response remains retained under the configured command-response
stream age and capacity limits; after eviction, the retained-response guard is
gone.

The same adapter is used across environments. Local and production deployments
use stable application scopes. Real-server tests create a globally unique
application scope, provision its normal messaging and event-store resources,
and delete the command, command-response, integration-event, quarantine, and
domain-event streams after the test.

## Consumer deadlines

Durable consumers configure separate deadlines for application processing and
broker acknowledgement. `processing_timeout` bounds one handler invocation;
`ack_wait` controls when JetStream may redeliver an unacknowledged message and
must be strictly greater than `processing_timeout`.

The NATS adapter sends progress acknowledgements while a handler is running and
once more before applying its final disposition. Applications should still
leave meaningful headroom for scheduling and acknowledgement latency, for
example a 30-second processing timeout with a 45-second ACK wait. Existing
durables must be reprovisioned with the new ACK wait before upgraded consumers
start, because runtime startup rejects mismatched durable policy.

Invalid deliveries are quarantined only after a confirmed PubAck. A transient
quarantine failure is retried up to the consumer's configured delivery limit;
an exhausted failure stops the consumer with the source message still pending
for operator recovery rather than redelivering it forever.

## Overrides

Replica counts are the normal deployment override. Application-bound low-level
stream and event-store constructors remain available for exceptional stream
names or policies. They reject subject filters outside the configured
application, and event-store subjects always retain bounded-context scope.
`ApplicationMessagingConfig::with_max_bytes` provides a bounded capacity
override without exposing subject filters.

## Security

Subject names are not a security boundary. Use NATS accounts and permissions.
Application-first subjects make those permissions direct: `fast-inbox.>` grants
access to one application's commands, queries, integration events, quarantine,
and domain-event subjects. Core NATS request/reply also uses `_INBOX.*` reply
subjects, so query clients and servers need the corresponding inbox publish and
subscribe permissions. Command-response reads are JetStream management API
requests, not ordinary subject subscriptions. In addition to command-response
subject permissions, readers need permission to request stream information and
raw messages for the configured response stream (the corresponding
`$JS.API.STREAM.INFO.*` and `$JS.API.STREAM.MSG.GET.*` subjects) and to subscribe
to their request inbox replies.
