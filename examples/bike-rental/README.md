# Bike-rental example

This application models a rental fleet and runs it through NATS-backed command
handlers, event storage, and Tracer. Start with the
[getting-started guide](https://elblabs.github.io/rostfrei/docs/getting-started)
for a walkthrough. Run the commands below from the repository root. Requirements:
the pinned Rust toolchain and NATS Server 2.12.1 or newer (the supplied Docker
Compose configuration provides NATS).

The application's only direct Rostfrei dependency is `rostfrei`, with `nats` and
`tracer` features plus `testing` for development. All framework imports use
`rostfrei::…`; see [dependency setup](../../docs/getting-started.mdx#one-framework-dependency).

## Completed-outcome cutover

NATS now defaults to a fail-closed legacy/read-only outcome namespace. For a **new
isolated application** (not an existing demo namespace), explicitly set both
`ROSTFREI_APPLICATION=bike-rental-epoch92` and
`ROSTFREI_COMPLETED_OUTCOME_EPOCH=epoch92` for provision/API processes. Fence old
credentials before starting new mutable workers. Programmatic fixtures use
`BikeRentalNatsConfig::with_fresh_completed_outcome_epoch` with
`BikeRentalNatsRuntime::provision_config`. This now selects the fresh two-stream
split authority, deriving disjoint journal/state namespaces. Do not use it to
reinterpret old retries or migrate histories implicitly; each belongs to its
original authority. The pair is verified by configuration and incarnation.
Old retries stay in their original namespace; a repaired business attempt uses a
new operation ID. See the [cutover runbook](../../docs/completed-outcome-cutover.md).
The [native authority contract](../../docs/command-runtime.md#native-authority) describes
explicit detail expiry, legacy scope mapping and authorized sender/worker APIs.

## Run locally

```sh
docker compose -f examples/bike-rental/compose.yaml up -d

ROSTFREI_NATS_URL=nats://127.0.0.1:4222 \
  ROSTFREI_APPLICATION=bike-rental-epoch92 \
  ROSTFREI_COMPLETED_OUTCOME_EPOCH=epoch92 \
  ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES=67108864 \
  ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES=268435456 \
  ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES=524288 \
  ROSTFREI_API_TOKEN=local-development-token \
  ROSTFREI_DISPATCH_TOKEN=local-dispatch-token \
  cargo run --locked -p bike-rental -- --tracer
```

Tracer listens at `http://127.0.0.1:1309`. Startup provisions local resources,
verifies the NATS topology, and seeds `city-fleet` with serviceable `bike-42` and
maintenance-required `bike-99`. Normal state persists across restarts; isolated
test state is recreated at startup when `--tracer` is enabled.

Omit `--tracer` to start the same Normal command runtime without HTTP tokens,
endpoints or correlation observers. Both modes reuse one executable binding
definition. See the [shared host API and measured reduction](../../docs/nats-application-host.md).

Use [Tracer Studio](../../studio/README.md) to inspect and run behavioral tests.

## Domain model

| Concept | Example |
| --- | --- |
| Aggregate | `RentalFleetAggregate` owns a fleet and its bicycles. |
| Commands | `RentBicycle`, `ReturnBicycle`, `AddBicycle`, and `TransferBicycle` belong to the `BikeRental` bounded context. |
| Domain service | `BicycleTransfer` coordinates a transfer between two fleets. |
| Policy | `RentalEligibilityPolicy` evaluates bicycle condition and whether the rental lifecycle permits renting. |
| Lifecycle | A bicycle can be available, rented, or retired. |
| Invariant | `FleetConsistency` demonstrates a rule over bicycle identities. |
| Value object | `RegistrationNumber` demonstrates local actions, policies, and invariants. |
| Query | `BicycleAvailabilityQuery` reads availability from fleet state. |

Command payloads carry all required fleet and bicycle IDs. `AddBicycle` takes
`fleet_id`, `bicycle_id`, and `condition`; it rejects an ID already in the fleet.
`RetireBicycleAction` demonstrates domain behavior but is not registered as a
command.

Print the compiled model without a broker:

```sh
cargo run --locked -p bike-rental --bin bike-rental-model
```

### Command execution

Handlers load aggregates through `CommandExecution` and call domain actions.
Loaded aggregates with new events become writing participants; unchanged loaded
aggregates become read guards. `CommandExecutor` commits all participants and
the acceptance receipt atomically at their expected stream versions and retries
the complete command attempt after a conflict. Event-free acceptance still
persists a receipt.

The [transfer handler](src/domain/bike_rental/bicycle_transfer/handler.rs) loads
the source and destination fleets. `BicycleTransfer` validates both before
raising `BicycleTransferredOut` and `BicycleTransferredIn`. The two commits
belong to one event transaction.

After `BicycleRented` commits, a durable domain-event consumer maps it to the
public `BicycleRentalStarted` integration event. `IntegrationEventBus` publishes
it, and a separate durable consumer handles it.

For the routing details, see [subjects and streams](https://elblabs.github.io/rostfrei/docs/messaging/subjects)
or the [same guide in this checkout](../../docs/messaging/subjects.mdx).

This development entry point explicitly provisions its NATS topology. The shared
host also offers a verify/connect-only startup for operator-provisioned resources.
It supervises command, domain-event, integration-event, observer and HTTP tasks,
and shuts down the owned group if any exits unexpectedly.

### Stable command subscription identities

Command consumer and durable names use `COMMAND_SUBSCRIPTION_GENERATION = 2`,
independently of `Command::SCHEMA_VERSION`. This preserves deployed names such as
`bike-rental--bike-rental--rent-bicycle--v2` and
`bike-rental--test--bike-rental--rent-bicycle--v2`. The suffix identifies the
retained subscription; it is not a payload or handler version. Domain-event and
integration-event subscriptions retain their `--v1` names.

Pin an application's subscriptions to their exact deployed identities. Keep an
existing `--v1` name rather than copying this example's `2`. Renaming a command
durable creates an overlapping WorkQueue subscription, so provisioning fails
even with no queued messages. Startup does not migrate conflicting durables.

For deliberate replacement, quiesce producers, handle pending work, stop the old
workers, and retire their durable before provisioning its replacement. A new
durable cannot replay commands already removed by acknowledgement. Stable names
preserve progress but do not make incompatible payloads readable: register
compatibility handlers for supported schemas. This example registers only its
current schemas. See [durable consumer versions](../../docs/messaging-upgrades.md#durable-consumer-versions).

## Quarantine walkthrough

The [quarantine walkthrough](QUARANTINE.md) checks retry exhaustion, inspection,
repair, republication, and acknowledgement against real NATS, including invalid
payloads and malformed transport messages:

```sh
ROSTFREI_NATS_URL=nats://127.0.0.1:4222 \
  cargo run --locked -p bike-rental --bin bike-rental-quarantine-demo
```

Start the supplied NATS server first. The walkthrough uses an isolated namespace
and cleans up afterward; add `-- --keep-streams` to retain it for inspection. It
runs independently of Tracer.

## Tracer

| Action | State | Authorization |
| --- | --- | --- |
| Test (`test`) | Publishes a command and appends to isolated test history. | Control token |
| Production dispatch (`dispatch`) | Uses the same pipeline against normal application state. | Dispatch token |

Discovery, test reset, and control-mode traces use `ROSTFREI_API_TOKEN`.
Production dispatch and its traces use `ROSTFREI_DISPATCH_TOKEN`. Startup rejects
equal tokens. Payloads are exposed in this local example; Tracer redacts them by
default.

### Observation and quarantine inspection

Tracer advertises Test quarantine through `catalog.quarantine.test.listHref`.
Set `ROSTFREI_INSPECTION_TOKEN` to enable separate read-only production
inspection; production payloads and metadata are redacted by default.

For inspection without application workers, run
`cargo run --locked -p bike-rental --bin bike-rental-quarantine` with the NATS URL
and inspection token configured. It listens on `127.0.0.1:1310` and does not
provision resources. See [quarantine inspection](../../docs/quarantine-inspection.md)
for filters, permissions, and reset behavior.

Studio's Observe panel follows externally triggered domain and integration
events. See [continuous observation](../../docs/continuous-observation.md) for
backend wiring, scope isolation, and retention.

### Test state and reset

The application has two traffic scopes:

- `bike-rental.test.>` uses separate command, response, integration-event,
  quarantine, and domain-event streams with the `BIKE_RENTAL__TEST` prefix.
- Normal subjects, such as `bike-rental.command.>`, retain state across test
  resets and application restarts.

`POST /test-scenario/reset` restores the `demo-fleet` fixture.
`POST /test-scenario/reset/{fixtureId}` selects another registered fixture.
Reset stops test workers, recreates test resources, replays the fixture's domain
events, and restarts the workers. A failed reset leaves Test, test
instances, and dynamic inputs unavailable until a reset succeeds.

The `demo-fleet` fixture builds `city-fleet` from two ordered `bicycle-added`
events. There is no legacy event decoder; incompatible demo histories require a
fresh `ROSTFREI_APPLICATION` namespace or deletion of disposable NATS streams.

For example, renting `bike-42` once succeeds. A second rental with a new
idempotency key is rejected with `BICYCLE_UNAVAILABLE`. Inspect that recorded
outcome without submitting again. Reset restores the initial state.

### Submission and observation

Test and production dispatch require an `Idempotency-Key`. A `202 Accepted`
response echoes the key and returns the resolved `operationId` and `Location`.
Follow `Location`: the key is not the operation ID. An exact retry returns the
retained operation; different content under the same key returns `409 Conflict`.
Reset rotates the test scenario generation so old evidence cannot enter a new
scenario. Retained operation resources do not expose the raw key.

Transported commands wait for NATS publication acknowledgement and a durable
accepted or rejected response. If publication was acknowledged but no valid
response can be established, the operation is `indeterminate`; that does not
prove the command failed. Do not automatically resubmit with a new key.

Operation resources advertise the observation links:

- `operationEventsHref`: operation-lifecycle SSE stream.
- `correlationEventsHref`: correlated business-message SSE stream.
- `messageSeriesHref`: finite capture of messages and command outcomes; accepts
  bounded `within` and `settleFor` windows.

Test and dispatch return observed evidence. Legacy Preview submission is retired.
For explicit rule diagnostics, follow `simulateHandlerHrefTemplate` and POST the
same command envelope. **Simulate handler** returns decision, starting versions
and proposed/uncommitted domain events using the registered handler over Test
history. It returns no operation or observed trace, and injected dependencies
are not automatically sandboxed. See [the interface](../../docs/handler-simulation.md).
An unsettled capture may be partial. Exact causal edges require a message's
`causationId` to match another message's `messageId`; timestamps and array order
do not establish causality. `grouped` fidelity means the capture cannot establish
all causal relationships.

Operations and traces are retained in count- and byte-bounded memory, not as a
durable audit log. Operation payloads share a 64 MiB budget; correlation evidence
has a separate 64 MiB budget divided evenly between control and dispatch modes.
Payloads exceeding a record's share are omitted. Retention is pressure-based.

### Behavioral tests

Definitions in [`tests/tracer`](tests/tracer) select a fixture from
[`fixtures`](fixtures) and describe an expected causal graph. Each run resets
the complete isolated scenario, replays the fixture's domain events, and
publishes the graph's root command through the Test pipeline. The server
compares observed messages and outcomes with the definition. Expected JSON
objects use subset matching; scalars and array lengths and order match exactly.

Catalog v1 advertises the links for listing, reading, validating, and running
tests. Executable behavioral-test documents use schema version 2. Catalog's
version is independent of document schema versions; the checked-in v1 schemas
are offline migration references.

### Agent workflow

The repository's Tracer skill discovers commands, tests, and inputs through
Catalog links. Start OpenCode from the repository root with the local target:

```sh
export ROSTFREI_TRACER_URL=http://127.0.0.1:1309
export ROSTFREI_API_TOKEN=local-development-token
export ROSTFREI_DISPATCH_TOKEN=local-dispatch-token
opencode
```

Example requests:

```text
List the available behavioral tests and summarize their intent.
Rent bike-42 from city-fleet in the agreed isolated Test scope and explain the observed flow.
Run rent-available-bicycle and explain the observed result.
```

The skill establishes the isolated execution/reset scope, reusing an already
agreed TDD scope. Production dispatch requires separately confirmed production inputs
and the dispatch token. Test-state discoveries are not production inputs.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `ROSTFREI_NATS_URL` | `nats://127.0.0.1:4222` | NATS connection. |
| `ROSTFREI_API_ADDR` | `127.0.0.1:1309` | HTTP listen address. |
| `ROSTFREI_APPLICATION` | `bike-rental` | Application namespace. |
| `ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES` | 64 MiB | Limit per messaging stream. |
| `ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES` | 10 GiB | Limit per domain-event stream. |
| `ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES` | 512 KiB | Event payload limit before transaction headers. |

Resource-limit variables accept positive byte counts. The local command above
limits each event store to 256 MiB to reduce JetStream capacity requirements.
Provisioning rejects malformed limits and mismatches with existing streams.
The process exits if a durable command, domain-event, or integration-event
consumer stops.

Deployed applications should provision infrastructure separately and use
distinct NATS credentials or accounts for Test and production dispatch.

## NATS integration checks

Run the example's tests with a disposable broker:

```sh
python3 scripts/test_nats.py -- cargo test --locked -p bike-rental -- --test-threads=1
```

Broker tests are enabled during normal Cargo test runs. To supply a disposable
NATS Server 2.12.1 or newer yourself:

```sh
ROSTFREI_NATS_URL=nats://127.0.0.1:4222 \
  ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES=67108864 \
  ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES=268435456 \
  ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES=524288 \
  cargo test --locked -p bike-rental \
  --test transfer --test nats_runtime_integration -- --test-threads=1
```

They cover cross-fleet transactions, command transport, scope isolation,
integration-command reactions, and five behavioral definitions. They fail if the
NATS URL is missing or unreachable, or the server version is unsupported.
