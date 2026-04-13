# Bike rental example

This public example models a bicycle rental fleet. It demonstrates rostfrei's
compiled domain metadata without depending on a production application:

- `RentalFleetAggregate` owns the fleet and its bicycles;
- `RentBicycle` is a public aggregate command;
- `AssessRentalEligibility` is a Rust decision;
- `BicycleRented` and `BicycleUnavailable` describe action outcomes; and
- `BicycleAvailabilityQueries` exposes a read-only availability query.

Print the compiled domain model:

```sh
cargo run --locked -p bike-rental
```

Run the example tests:

```sh
cargo test --locked -p bike-rental
```

## Simulation API

Run the local control-plane server:

```sh
ROSTFREI_API_TOKEN=local-development-token \
  cargo run --locked -p bike-rental --bin bike-rental-api
```

It binds to `127.0.0.1:3000` by default. Set `ROSTFREI_API_ADDR` to use another
local address. `ROSTFREI_API_TOKEN` is required and protects every endpoint with
a bearer capability. The demo seeds `city-fleet` with serviceable `bike-42` and
maintenance-required `bike-99`.

Submit a simulation:

```sh
curl --request POST \
  http://127.0.0.1:3000/v1/contexts/bike-rental/aggregates/rental-fleet/city-fleet/commands/bike-rental.rent-bicycle/simulate \
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
never publishes to NATS. Operation traces are bounded and in-memory, so this
server is a local development example rather than a durable live dispatch
endpoint.
