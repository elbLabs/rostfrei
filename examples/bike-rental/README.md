# Bike rental example

This public example models a bicycle rental fleet. It demonstrates rostfrei's
compiled domain metadata without depending on a production application:

- `RentalFleetAggregate` owns the fleet and its bicycles;
- `RentBicycle` is a public aggregate command;
- `AssessRentalEligibility` returns an allowed outcome or a typed denial reason;
- `BicycleRented` and `BicycleUnavailable` describe action outcomes; and
- `BicycleAvailabilityQueries` exposes a read-only availability query.

The public command maps its payload to the `BicycleId` domain identity. The
modeled Action returns `BicycleRented`; a generated aggregate-instance adapter
raises and applies that event only when the Action succeeds. Neither the Action
nor the Decision receives the command message, and application code does not
call `AggregateInstance::raise`.

Print the compiled domain model:

```sh
cargo run --locked -p bike-rental --bin bike-rental-model
```

Run the example tests:

```sh
cargo test --locked -p bike-rental
```

## NATS command lab

The runnable example uses the same NATS publication, durable command consumer,
and `NatsEventStore` path intended for deployed systems. NATS Server 2.12 or
newer is required for atomic event batches. Start a disposable local server:

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
Runtime startup verifies the operator-provisioned NATS topology and exits if the
durable command consumer stops.

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

Stream the publication trace:

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

The dispatch trace reports `command.published` only after a confirmed JetStream
PubAck. Publication is not aggregate acceptance: the durable worker consumes
the envelope, recomputes its operation fingerprint, executes `RentBicycle`, and
appends accepted events to the `city-fleet` stream. Reusing an
`Idempotency-Key` returns the retained operation for the exact same request and
returns `409 Conflict` for different content.
The NATS adapter makes bounded retries for publication timeouts and broker
unavailability with the same content-scoped message identity before reporting a
terminal failure.

The broker deduplication identity includes both the operation identity and the
request fingerprint, so a duplicate PubAck represents an exact wire retry, not
different content submitted under a reused operation identity.

Dispatching `bike-42` again with another idempotency key publishes another real
command. The worker replays `BicycleRented`, rejects the second rental as
`BicycleUnavailable`, acknowledges that business outcome, and appends no event.

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
and deletes all four JetStream streams after the run:

```sh
ROSTFREI_NATS_URL=nats://127.0.0.1:4222 \
  cargo test --locked -p bike-rental --test nats_dispatch_integration
```
