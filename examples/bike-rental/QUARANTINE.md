# Quarantine walkthrough

Run the complete retry → quarantine → inspect → repair → republish → acknowledge
flow against real NATS JetStream:

```sh
docker compose -f examples/bike-rental/compose.yaml up -d

export ROSTFREI_NATS_URL=nats://127.0.0.1:4222
cargo run --locked -p bike-rental --bin bike-rental-quarantine-demo
```

Run these commands from the repository root. NATS Server 2.12.1 or newer is
required. The demo creates a unique `bike-quarantine-<uuid>` application in Test
scope on each run, limits each messaging stream to 8 MiB, and removes its streams
when finished. The Tracer server does not need to be running.

`bike-rental-quarantine-demo` runs the walkthrough. The separate
`bike-rental-quarantine` binary serves the read-only production inspection API;
see [quarantine inspection](../../docs/quarantine-inspection.md).

## What happens

[`src/bin/bike-rental-quarantine-demo.rs`](src/bin/bike-rental-quarantine-demo.rs) uses
Rostfrei's `NatsPublisher`, `NatsConsumerFactory`, and durable consumer. Its small
`notify-rental` handler simulates an external notification service for
`city-fleet/bike-42`. The service outage and repair are simulated; all broker
publications, redeliveries, quarantine records, and acknowledgements are real.
This is a transport-level example with its own handler and payload contract.

1. **Retry exhaustion.** The notification service is unavailable. The handler
   returns `DeliveryDisposition::RetryAfter(250ms)`. Deliveries 1 and 2 are
   negatively acknowledged with that delay. On delivery 3, the consumer reaches
   its configured `maximum_delivery_attempts` and publishes a quarantine record
   with reason `maximum delivery attempts exceeded` and `failure_kind`
   `delivery-attempts-exhausted`.
2. **Inspection and recovery.** The demo reads and prints that durable record
   and its decoded payload. It repairs the simulated service and explicitly
   republishes the original payload with a **new message ID**, preserved caller
   metadata, and `x-redrive-of` pointing to the original ID. The new command is
   acknowledged successfully. The original quarantine record remains available.
3. **Immediate application quarantine.** A JSON payload missing `bicycle_id`
   reaches the handler, which returns `DeliveryDisposition::Quarantine` with a
   specific reason. It is quarantined on the first attempt with `failure_kind`
   `handler-failure`.
4. **Invalid transport message.** A raw JetStream publication omits the required
   `Content-Type` and `Nats-Msg-Id` headers. The adapter quarantines it with reason
   `invalid source message` and `failure_kind` `invalid-source-message`, without
   calling the application handler.

For every quarantine path, Rostfrei waits for the quarantine publication's
**PubAck before sending TERM** to the original delivery. In this command
WorkQueue, ACK or TERM removes the source message. The demo checks that the
source queue drains, the expected records exist, retry counts, reasons, and
failure categories match, and the recovered notification succeeds. It exits
nonzero on a failed check and prints this on success:

```text
PASS: recovery acknowledged, source queue empty, three quarantine records retained.
Removed this run's demo streams.
```

## Inspect with the NATS CLI

Keep the records after the walkthrough:

```sh
cargo run --locked -p bike-rental --bin bike-rental-quarantine-demo -- --keep-streams
```

The binary prints the exact stream names, a `nats stream view` command, and
cleanup commands for that run. No NATS CLI is needed for the walkthrough itself.
`stream view` requires an interactive terminal. For scripts, use
`nats --server "$ROSTFREI_NATS_URL" stream get <printed-quarantine-stream> 1`
to read the first record; sequences 2 and 3 contain the malformed-message cases.

The quarantine subject is:

```text
<application>.test.quarantine.command.bike-rental.notify-rental
```

Each record includes the original message ID and address, base64-encoded payload,
payload size and SHA-256, a truncation flag, caller metadata, optional trace
context, reason, structured failure category, delivery attempt, and source
stream/consumer sequences and names. These demo payloads are retained in full.
Oversized records may retain only a bounded payload prefix; a truncated record
is not a complete recovery payload.

## Recovery semantics and other failure paths

- **Republication is explicit application code.** Reading a quarantine record
  does not redrive it. The demo demonstrates how to recover its simple
  notification payload. A typed command envelope also has
  operation/idempotency identities that must be considered before resubmission.
- **Use a new message ID for this recovery.** Reusing the old ID within the
  JetStream duplicate window can deduplicate the publication instead of creating
  a new delivery. A fresh publication starts a new delivery-attempt budget.
  Recovery handlers should still be idempotent: a failed delivery may have made
  an external change before failing.
- **Domain rejection is a normal outcome.** For example, renting an unavailable
  bicycle produces a rejected command response. That is different from a
  transport/handler failure that needs retry or quarantine.
- **Processing timeout also consumes the retry budget.** It retries with the
  adapter's default delay and eventually uses the same exhaustion reason.
- **If quarantine publication fails, the source is not terminated.** The adapter
  retries while the delivery budget allows. At the limit it returns a quarantine
  error and stops the consumer, leaving the source unacknowledged for recovery.
  These last two failure paths are described here rather than injected by the
  walkthrough.
- **Domain-event consumers have a separate failure policy.** They stop on an
  incomplete projection/publication instead of automatically quarantining and
  continuing. See [ADR 0012](../../docs/adr/0012-domain-event-handlers.md).

## Automated check

The same self-checking walkthrough runs as a regular test in the workspace's
real-NATS CI suite. Run just that test with a disposable broker:

```sh
python3 scripts/test_nats.py -- \
  cargo test --locked -p bike-rental --bin bike-rental-quarantine-demo -- --nocapture
```

Or use an existing broker:

```sh
ROSTFREI_NATS_URL=nats://127.0.0.1:4222 \
  cargo test --locked -p bike-rental --bin bike-rental-quarantine-demo \
  -- --nocapture
```

The test requires the URL to be set and fails on an unavailable server. It
cleans up its unique streams. The framework also has an existing lower-level
test named `durable_consumer_applies_ack_retry_and_puback_before_quarantine_term`
in `crates/rostfrei-nats/tests/messaging_integration.rs`.
