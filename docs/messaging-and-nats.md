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
| `FAST_INBOX_INTEGRATION_EVENTS` | `fast-inbox.integration.>` | Limits |
| `FAST_INBOX_QUARANTINE` | `fast-inbox.quarantine.>` | Limits |

The bounded-context event store derives:

```text
FAST_INBOX__COMMERCIAL_ACCESS_DOMAIN_EVENTS
fast-inbox.domain.commercial-access.>
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
subscribe permissions.
