# Bike rental example

This public example models a bicycle rental fleet. It demonstrates rostfrei's
compiled domain metadata without depending on a production application:

- `RentalFleetAggregate` owns the fleet and its bicycles;
- `RentBicycle` is a public aggregate command;
- `RentalEligibilityDecisions` groups `AssessRentalEligibility`, which returns
  first-class eligible, already-rented, or maintenance-required outcomes;
- `BicycleRented` and `BicycleUnavailable` describe action outcomes; and
- `BicycleAvailabilityQueries` exposes a read-only availability query.

The public command maps its payload to the `BicycleId` domain identity. The
modeled Action exhaustively translates the eligibility outcome, explicitly
raising `BicycleRented` on success while preserving `BicycleUnavailable` as its
external error. The Decision takes the compact status and condition values;
neither the Action nor the Decision receives the command message.

Print the compiled domain model:

```sh
cargo run --locked -p bike-rental --bin bike-rental-model
```

Run the example tests:

```sh
cargo test --locked -p bike-rental
```

## NATS command lab

The runnable example uses the shared `CommandBus`, `IntegrationEventBus`, NATS
adapters, durable command and domain-event consumers, immutable command
responses, and `NatsEventStore` path intended for deployed systems. After
`BicycleRented` commits, the domain-event consumer maps it to the public
`BicycleRentalStarted` integration event; a separate durable consumer handles
that event. NATS Server 2.12 or newer is required for atomic event batches.
Start a disposable local server:

```sh
docker compose -f examples/bike-rental/compose.yaml up -d
```

The Compose volume preserves local streams across server restarts. Use
`docker compose -f examples/bike-rental/compose.yaml down -v` for a full reset.
Compose publishes the unauthenticated development broker on loopback only.

Provision the application-scoped messaging streams, command durable, and event
store, then seed `city-fleet`:

```sh
cargo run --locked -p bike-rental --bin bike-rental-provision
```

Provisioning is idempotent and separate from runtime startup. The command uses
`nats://127.0.0.1:4222` and application `bike-rental-demo` by default. Override
them with `ROSTFREI_NATS_URL` and `ROSTFREI_APPLICATION` in local or production
environments.

Run the API and command worker:

```sh
ROSTFREI_API_TOKEN=local-development-token \
ROSTFREI_DISPATCH_TOKEN=local-development-token \
  cargo run --locked -p bike-rental
```

It binds to `127.0.0.1:3000` by default. Set `ROSTFREI_API_ADDR` to use another
address. `ROSTFREI_API_TOKEN` protects simulation and operation traces;
`ROSTFREI_DISPATCH_TOKEN` is the separate live-command capability. They may be
equal for local development but should be distinct in deployed environments.
Operation resources and traces require `ROSTFREI_API_TOKEN`, including for an
operation created with `ROSTFREI_DISPATCH_TOKEN`.
Runtime startup verifies the operator-provisioned NATS topology and exits if a
durable command, domain-event, or integration-event consumer stops.

The fleet contains serviceable `bike-42` and maintenance-required `bike-99`.
This local server explicitly exposes trace payloads for demonstration; the
control-plane library redacts them by default.

Open <http://127.0.0.1:3000> to select and submit the demo command, edit its
payload, and watch the operation events stream into the page. Enter the
`local-development-token` used in the command above when the page asks for the
bearer token.

Dispatch a real command through NATS:

```sh
curl --request POST \
  http://127.0.0.1:3000/v1/contexts/bike-rental/aggregates/rental-fleet/city-fleet/commands/rent-bicycle/dispatch \
  --header 'content-type: application/json' \
  --header 'authorization: Bearer local-development-token' \
  --header 'idempotency-key: rental-operation-1' \
  --data '{"schemaVersion":1,"payload":{"bicycle_id":"bike-42"}}'
```

Stream the command-completion trace:

```sh
curl --no-buffer \
  --header 'authorization: Bearer local-development-token' \
  http://127.0.0.1:3000/v1/operations/rental-operation-1/events
```

Retrieve the current or final operation resource:

```sh
curl --header 'authorization: Bearer local-development-token' \
  http://127.0.0.1:3000/v1/operations/rental-operation-1
```

The dispatch trace reports `command.published` immediately after the command
JetStream PubAck but remains running. The durable worker consumes the envelope,
recomputes its operation fingerprint, executes `RentBicycle`, derives the exact
response address, and publishes an immutable accepted or rejected response. It
ACKs the command only after the response PubAck. Before execution, the worker
checks that exact response address; a matching retained response is ACKed without
running the aggregate again. Store unavailability retries without execution,
while an invalid or conflicting response is quarantined. The response uses its
own v1 schema and carries the originating command address.

The adapter reads responses in bounded slices and keeps listening
through slice timeouts or transient reader unavailability until a response
arrives or the operation task is cancelled. It then reports
`command.responded`, the business outcome, and terminal completion. Accepted
execution appends events to the `city-fleet` stream, but the dispatch result
omits `appended` because the response contains no authoritative append evidence;
simulation continues to report `appended: false`. Reusing an `Idempotency-Key`
returns the retained operation for the exact same request and returns
`409 Conflict` for different content.
The NATS adapter makes bounded retries for command publication timeouts and
broker unavailability with the same content-scoped message identity. The worker
also retries transient response publication without acknowledging the command.

The broker deduplication identity includes both the operation identity and the
request fingerprint, so a duplicate PubAck represents an exact wire retry, not
different content submitted under a reused operation identity.

Dispatching `bike-42` again with another idempotency key publishes another real
command. The worker replays `BicycleRented`, rejects the second rental as
`BicycleUnavailable`, durably publishes that rejection before acknowledging the
command, and appends no event. Both command responses remain observable in the
application's command-response stream.

This flow does not promise exactly-once terminal decisions. Event-appending
acceptance can recover by exact event-store replay, but rejected and
accepted-no-event decisions have a crash window between deciding and persisting
their response because there is no transactional operation receipt or outbox.
A redelivery can evaluate those decisions again. Response immutability and the
pre-execution reconciliation guard last only while the response is retained
under the configured response-stream age and capacity limits.

Submit a non-mutating simulation by changing the route suffix to `/simulate`.
Simulation reports replay, acceptance or rejection, predicted events, and
completion. It never appends or publishes. After the real dispatch above, a
simulation for `bike-42` is rejected because it reads the same NATS-backed
aggregate stream. Reconnect to either operation trace with `Last-Event-ID`; a
cursor at the terminal event returns `204 No Content`.

The route uses context-qualified aggregate identity
`bike-rental/rental-fleet`. `RentBicycle`, its rejection, and aggregate events
use generated JSON contracts rather than handwritten bike-rental codecs.

Operation resources and traces are count-bounded and in-memory, simulation and
dispatch admission are bounded independently, and operation retention is
pressure-based rather than durable or time-based. Production deployments must
add durable audit and operation-outcome storage appropriate to their security
model. Durable enforcement of idempotency-key conflicts across replicas or
beyond in-memory retention belongs in that operation store.

## Real NATS test

Fast tests use fakes and `InMemoryEventStore`. The real-server test uses the same
NATS adapter and worker as runtime, creates a globally unique application scope,
and deletes all five JetStream streams after the run:

```sh
ROSTFREI_NATS_URL=nats://127.0.0.1:4222 \
  cargo test --locked -p bike-rental --test nats_dispatch_integration
```
