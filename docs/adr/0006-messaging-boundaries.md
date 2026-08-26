# ADR 0006: Messaging transport boundaries

## Status

Accepted; address and topology ownership partially superseded by ADR 0015.

## Decision

`rostfrei-messaging-core` owns validated command, integration-event, and query
addresses; bounded envelopes; correlation and causation metadata; publishing
ports; consumer dispositions; and stable query error classifications. It has no
broker dependency.

`rostfrei-nats` owns connection lifecycle, JetStream publication with PubAck
confirmation, durable pull consumers, ACK/NAK/term translation, Core NATS
request/reply, queue groups, retry and quarantine mechanics, broker header
validation, and generic provisioning.

Adapters exclusively own control headers, including reply subjects,
`Nats-Msg-Id`, expected-version/stream headers, content type, and consumer ACK
protocol. Caller metadata cannot override them. Queries use adapter-generated
Core NATS inboxes and never write to JetStream.

## Consequences

Applications choose business addresses and delivery policy without importing
`async-nats`. Broker-specific acknowledgements and errors remain available from
the adapter where operational code needs them, while ordinary application ports
stay transport-neutral.
