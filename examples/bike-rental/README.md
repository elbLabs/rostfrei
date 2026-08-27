# Bike rental example

This public example models a bicycle rental fleet. It demonstrates rostfrei's
compiled domain metadata without depending on a production application:

- `RentalFleetAggregate` owns the fleet and its bicycles;
- `RentBicycle` is a public aggregate command;
- `AssessRentalEligibility` returns an allowed outcome or a typed denial reason;
- `BicycleRented` and `BicycleUnavailable` describe action outcomes; and
- `BicycleAvailabilityQueries` exposes a read-only availability query.

The public command maps its payload into `RentBicycleInput`. The modeled Action
returns `BicycleRented`; a generated aggregate-instance adapter raises and
applies that event only when the Action succeeds. Neither the Action nor the
Decision receives the command message, and application code does not call
`AggregateInstance::raise`.

Print the compiled domain model:

```sh
cargo run --locked -p bike-rental --bin bike-rental-model
```

Run the example tests:

```sh
cargo test --locked -p bike-rental
```

## Simulation API

Run the local control-plane server:

```sh
ROSTFREI_API_TOKEN=local-development-token \
  cargo run --locked -p bike-rental
```

It binds to `127.0.0.1:3000` by default. Set `ROSTFREI_API_ADDR` to use another
local address. `ROSTFREI_API_TOKEN` is required and protects every API endpoint
with a bearer capability. The demo seeds `city-fleet` with serviceable `bike-42`
and maintenance-required `bike-99`. This local server explicitly exposes trace
payloads for demonstration; the control-plane library redacts them by default.

Open <http://127.0.0.1:3000> to select and submit the demo command, edit its
payload, and watch the operation events stream into the page. Enter the
`local-development-token` used in the command above when the page asks for the
bearer token.

Submit a simulation:

```sh
curl --request POST \
  http://127.0.0.1:3000/v1/contexts/bike-rental/aggregates/rental-fleet/city-fleet/commands/rent-bicycle/simulate \
  --header 'content-type: application/json' \
  --header 'authorization: Bearer local-development-token' \
  --header 'idempotency-key: rental-operation-1' \
  --data '{"schemaVersion":1,"payload":{"bicycle_id":"bike-42"}}'
```

Stream its trace:

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

The trace reports replay, command acceptance or rejection, predicted domain
events, and completion. Simulation never appends to the aggregate stream and
never publishes to NATS. Reconnect with `Last-Event-ID` to resume after a
previous SSE event; a cursor at the terminal event returns `204 No Content`.
Reusing an `Idempotency-Key` returns the retained operation only for the exact
same request and returns `409 Conflict` for different content.

The route uses the context-qualified aggregate identity
`bike-rental/rental-fleet`. The in-memory demo has no NATS application scope; a
deployment using `NatsEventStore` supplies one application-and-context-scoped
history. `RentBicycle`, its rejection, and aggregate events use generated JSON
contracts rather than handwritten bike-rental codecs.

Operation resources and traces are count-bounded and in-memory, concurrent
simulation admission is bounded, and retention is pressure-based rather than
durable or time-based. This server is therefore a local development example,
not a durable live dispatch endpoint.
