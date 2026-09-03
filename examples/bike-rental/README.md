# Bike rental example

This public example models a bicycle rental fleet. It demonstrates rostfrei's
compiled domain metadata without depending on a production application:

- `RentalFleetAggregate` owns the fleet and its bicycles;
- `RentBicycle`, `ReturnBicycle`, and `AddBicycle` are public aggregate commands;
- `RentalEligibilityDecision` is a Rust decision;
- fleet-imported, bicycle-added, rented, and returned events describe successful transitions;
- unavailable and not-rented errors describe command rejections;
- `FleetConsistency` detects duplicate bicycle identities;
- `RegistrationNumber` demonstrates Value Object-local actions, invariants, and decisions; and
- `BicycleAvailabilityQuery` exposes a read-only availability query.

Print the compiled domain model:

```sh
cargo run --locked -p bike-rental --bin bike-rental-model
```

Run the example tests:

```sh
cargo test --locked -p bike-rental
```

## NATS-backed Tracer

The runnable example uses the shared `CommandBus`, `IntegrationEventBus`, NATS
adapters, durable command and domain-event consumers, immutable command
responses, and `NatsEventStore` path intended for deployed systems. After
`BicycleRented` commits, the domain-event consumer maps it to the public
`BicycleRentalStarted` integration event; a separate durable consumer handles
that event. NATS Server 2.12.1 or newer is required for atomic event batches.
Start the supplied disposable NATS server and local Tracer:

```sh
docker compose -f examples/bike-rental/compose.yaml up -d

ROSTFREI_NATS_URL=nats://127.0.0.1:4222 \
  ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES=67108864 \
  ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES=268435456 \
  ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES=524288 \
  ROSTFREI_API_TOKEN=local-development-token \
  ROSTFREI_DISPATCH_TOKEN=local-dispatch-token \
  cargo run --locked -p bike-rental
```

It binds to `127.0.0.1:1309` by default. Set `ROSTFREI_API_ADDR` to use another
local address. The control capability protects discovery, simulation, isolated
test execution, reset, and their traces. The separate dispatch capability is
required for dispatch execution and its traces; startup rejects equal tokens.
The canonical application identity defaults to `bike-rental`. Set
`ROSTFREI_APPLICATION` to override that one base token when isolating multiple
instances; Rostfrei never appends a deployment label to it.

The example accepts byte-count resource limits through the environment:

- `ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES` limits each messaging stream and
  defaults to 64 MiB;
- `ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES` limits each authoritative
  domain-event stream and defaults to 10 GiB; and
- `ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES` limits an event payload before
  atomic-transaction headers and defaults to 512 KiB.

The local command above explicitly limits each Test and normal event store
to 256 MiB so NATS does not need to reserve more than 20 GiB of JetStream
capacity. Provisioning rejects non-positive or malformed values and detects
limits that disagree with already-provisioned streams.

Runtime startup verifies the operator-provisioned NATS topology and exits if a
durable command, domain-event, or integration-event consumer stops.

The example uses one canonical application with two disjoint traffic scopes:

- `bike-rental.test.>` is recreated and deterministically seeded on startup and
  by `POST /test-scenario/reset`;
- normal `bike-rental` subjects such as `bike-rental.command.>` persist across
  restarts and are never affected by test reset.

Each scope has separate command, command-response, integration-event,
quarantine, and authoritative domain-event streams, plus separate durables.
Test resources use the `BIKE_RENTAL__TEST` stream prefix. Test reset stops its
workers, recreates that complete topology, reseeds it, and restarts the workers
without touching normal Dispatch resources.
A failed reset leaves Test, Simulate, instances, and dynamic inputs unavailable
until a later reset succeeds rather than exposing partially rebuilt state.

Both initially contain `city-fleet`, serviceable `bike-42`, and
maintenance-required `bike-99`. The local example explicitly provisions these
streams and exposes trace payloads for demonstration. Production deployments
should provision infrastructure separately and use distinct NATS credentials or
accounts for Test and Dispatch.

The API advertises all command fields, runtime choices, instances, mode actions,
and reset links through `GET /catalog` and its linked resources. Clients can
follow these links without embedding bike-rental values.

Behavioral tests are YAML files in `tests/tracer`. They name the deterministic
`demo-fleet` fixture, run setup and subject commands through the isolated test
NATS pipeline, and assert the command outcome plus correlated domain or
integration events. The filesystem remains the source of truth:

```sh
curl --header 'authorization: Bearer local-development-token' \
  http://127.0.0.1:1309/tests

curl --request POST \
  --header 'authorization: Bearer local-development-token' \
  http://127.0.0.1:1309/tests/rent-available-bicycle/runs
```

Run the dispatch-isolation check and all three behavioral definitions against
a real NATS server:

```sh
ROSTFREI_NATS_URL=nats://127.0.0.1:4222 \
  ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES=67108864 \
  ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES=268435456 \
  ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES=524288 \
  cargo test --locked -p bike-rental \
  --test nats_runtime_integration
```

Submit a simulation:

```sh
curl --request POST \
  http://127.0.0.1:1309/contexts/bike-rental/aggregates/rental-fleet/city-fleet/commands/rent-bicycle/simulate \
  --header 'content-type: application/json' \
  --header 'authorization: Bearer local-development-token' \
  --header 'idempotency-key: rental-operation-1' \
  --data '{"schemaVersion":1,"payload":{"bicycle_id":"bike-42"}}'
```

Stream its correlated business flow:

```sh
curl --no-buffer \
  --header 'authorization: Bearer local-development-token' \
  http://127.0.0.1:1309/correlations/rental-operation-1/events
```

Retrieve the current or final operation resource:

```sh
curl --header 'authorization: Bearer local-development-token' \
  http://127.0.0.1:1309/operations/rental-operation-1
```

The three Tracer actions have distinct semantics:

- `simulate` replays the current test NATS history but never appends;
- `test` publishes through the normal command pipeline and appends only to the
  isolated test NATS history;
- `dispatch` uses the identical pipeline against the canonical unsuffixed
  application namespace.

Both transported modes wait for command PubAck and a durable accepted or
rejected response. Accepted rentals then flow through a durable post-commit
domain-event handler, publish the correlated `bicycle-rental-started`
integration event, and consume it through the normal integration-event durable.
An `Idempotency-Key` is required for Test and Dispatch. If an error occurs after
PubAck but before a valid durable response is observed, the operation is
`indeterminate` and preserves its command message identity instead of claiming
that the business command failed.

Running Test for `bike-42` twice accepts and appends the first command, then
replays that new state and rejects the second with `BICYCLE_UNAVAILABLE`.
Simulate subsequently observes the same rejection without changing history.
Reset returns the test stream to the deterministic seed and does not touch
Dispatch state.

`ReturnBicycle` advertises currently rented bicycles as runtime input choices and
makes the selected bicycle available again. `AddBicycle` has no user-supplied
payload. The aggregate assigns the next unused deterministic UUID and adds the
bicycle as available and serviceable. All three commands use their generated
JSON payload contracts; Tracer has no command-specific wire codecs.

Correlation feeds report the command, observed domain events, observed integration
events, and the command result. They remain open for downstream integration events;
reconnect with `Last-Event-ID` to replay retained events after a previous SSE frame.
Operation traces remain available at `/operations/{operationId}/events` for
Tracer lifecycle details.
Reusing an `Idempotency-Key` returns the retained operation only for the exact
same request and returns `409 Conflict` for different content.
Each Test reset rotates the Test scenario generation, so a key reused afterward
cannot receive delayed correlation events from the previous scenario.

The route uses the context-qualified aggregate identity
`bike-rental/rental-fleet`. `RentBicycle`, its rejection, and aggregate events
use generated JSON contracts rather than handwritten bike-rental codecs.

Operation resources, traces, and correlation feeds remain count- and
byte-bounded in memory even though domain events are durable in NATS. Operation
and correlation payload retention each have a 64 MiB aggregate budget; payloads
that cannot fit a record's share are omitted rather than retained without bound.
Concurrent admission is bounded, and operation retention is pressure-based
rather than durable or time-based. This server is therefore a local development
example, not a production audit system.
