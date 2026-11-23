# Bike-rental example

This application models category reservations with fixed rental quotes, sandbox
payments and deposit refunds, and damaged returns handed to a workshop. It runs through real NATS-backed command
handlers, event storage, and Tracer. Start with the
[getting-started guide](https://elblabs.github.io/rostfrei/docs/getting-started)
for a walkthrough. Run the commands below from the repository root. Requirements:
the pinned Rust toolchain and NATS Server 2.12.1 or newer (the supplied Docker
Compose configuration provides NATS).

The application's only direct Rostfrei dependency is `rostfrei`, with `nats` and
`tracer` features plus `testing` for development. All framework imports use
`rostfrei::…`; see [dependency setup](../../docs/getting-started.mdx#one-framework-dependency).

## Customer showcase

With the pinned Rust toolchain and Docker running, launch a self-checking walkthrough:

```sh
cargo xtask test -- cargo run --locked -p bike-rental --bin bike-rental-showcase
```

The runner starts a disposable NATS broker. The showcase uses a fresh Test
application and `reservable-branch` revision 3: categorized city bikes and a
tariff with a refundable deposit. It stops its workers and removes
its streams afterward, retaining evidence under the printed
`target/bike-rental-showcase/<run>/` directory.

| Moment | What the customer sees | What Rostfrei demonstrates |
| --- | --- | --- |
| Book ahead | Alex receives a €25 rental quote and a €100 refundable deposit amount. | Category capacity and the fixed quote are recorded together. |
| Collect and pay | Staff selects bike-42; sandbox approval starts the rental. | A durable hold/request precedes payment; finance and rental completion share one operation receipt. |
| Report faulty brakes at return | The return is accepted and damage is recorded. | Rental completion and the maintenance condition commit atomically. |
| Workshop takes over automatically | A repair job contains the bike, fleet and reported fault. | A committed domain event becomes a public notification, then a typed command to another aggregate. |
| Repeat the same request | The same operation is returned; there is still one repair job. | Idempotent execution protects business effects. |
| Try to reserve again with no serviceable capacity | `RESERVATION_REJECTED`. | Bicycles awaiting repair cannot support a new booking. |
| Register and transfer a spare | `bike-77` moves from the harbor to the city station. | Both fleets change in one atomic event transaction. |
| Return the next rental on time | Sam receives the full €100 deposit refund; the €25 rental charge stays paid. | A committed return triggers an idempotent sandbox refund to the original charge. |
| Confirm the damaged-return deposit remains held | Alex's deposit stays held. | Recording the physical return is separate from releasing its deposit. |

Open `reservation.json`, `collection.json`, `return.json` and `blocked-rental.json`
to inspect operations and Tracer captures. `payment.json` and `repair-job.json`
contain independently read persisted decisions;
`transfer.json` captures the two-station transfer. `satisfactory-return.json` and
`refund.json` show the second customer's return and confirmed deposit refund.
Live captures include automatic payment, workshop and settlement reactions; their
generated commands are not captured nodes, so these captures have grouped
fidelity. Explain the recorded correlation and explicit causation IDs using the
[observation contract](../../docs/continuous-observation.md).

For an interactive presentation, start the API below, reset isolated Test to
`rental-workshop`, and submit `return-bicycle` schema 2 with:

```json
{
  "fleet_id": "city-fleet",
  "bicycle_id": "bike-42",
  "damage_report": "Rear brake does not engage"
}
```

Inspect the return and the resulting repair job. Use [the guided agent workflow](../../docs/agent-tutorial.md)
to explore the recorded messages and source rules with the customer.

### Present in Studio

Start the [local API](#run-locally), then start Studio from another terminal using
the matching local control and inspection tokens:

```sh
pnpm install --frozen-lockfile
VITE_TRACER_TARGET=http://127.0.0.1:1309 \
  VITE_TRACER_TOKEN=local-development-token \
  VITE_TRACER_INSPECTION_TOKEN=local-inspection-token \
  pnpm --dir studio dev --host 127.0.0.1
```

Open the URL Vite prints. Select **Domain** to explore the bike-rental model.
In **Tests → Run a test**, choose these scenarios for a
short customer presentation:

1. **Confirm a category reservation with payment due at collection**
   (`reserve-city-bike`): inspect the confirmed quote and deposit.
2. **Approve sandbox payment and hand over the reserved bicycle**
   (`approve-collection-payment`): inspect payment approval and rental start.
3. **Refund the full deposit after a satisfactory on-time return**
   (`refund-security-deposit`): inspect the amount and original charge reference.
4. **Workshop opens a repair job for a damaged bicycle** (`open-repair-job`):
   inspect the recorded fault.

The library contains seven tests. Each **Run** resets the complete isolated Test
scenario to its own fixture, then executes real handlers through NATS. Selecting
a library entry only previews its expected flow. **Runs → Run summary** shows a
completed test's verdict and evidence; select graph cards to inspect payloads.

For a continuous journey, run `reserve-city-bike` once, then use **Commands → Send
command → Isolated Test** for the [collection](#collection-and-sandbox-payment) and
[return](#returns-and-deposit-settlement) payloads below. Open **Live flows** and
choose the **Test** scope to see the automatic payment, refund or
workshop reactions. Run another test when ready to reset the demonstration.
Observe shows domain and integration events, not commands. Use the command result
or test report for the initiating decision; automatic effects can have grouped fidelity.

In manual forms, category and timestamp values use JSON strings, for example
`"city"` and `"2026-12-01T17:00:00Z"`. Supply `returned_at` to demonstrate an
on-time refund; an unknown return time retains the deposit. Use `sandbox-approved`
as the payment method for approval; other values decline without cancelling the booking.

Verify all scenarios through the actual browser and host with:

```sh
cargo xtask test -- cargo xtask acceptance bike-rental-host --studio
```

## Completed-outcome cutover

For a **new isolated application** (not an existing demo namespace), set
`ROSTFREI_APPLICATION=bike-rental-demo` for provision/API processes. Both binaries
select label-free journal/command-results storage by default. Programmatic fixtures
use `BikeRentalNatsConfig::with_fresh_outcomes` with
`BikeRentalNatsRuntime::provision_config`. Fence old credentials before starting
new mutable workers. Do not use this fresh setup to
reinterpret old retries or migrate histories implicitly; each belongs to its
original authority. The pair is verified by configuration and incarnation.
For an existing named-epoch store, retain its explicit
`ROSTFREI_COMPLETED_OUTCOME_EPOCH` value; removing it selects different storage.
Old retries stay in their original namespace; a repaired business attempt uses a
new operation ID. See the [cutover runbook](../../docs/completed-outcome-cutover.md).
The [native authority contract](../../docs/command-runtime.md#native-authority) describes
explicit detail expiry, legacy scope mapping and authorized sender/worker APIs.

## Run locally

```sh
docker compose -f examples/bike-rental/compose.yaml up -d
unset ROSTFREI_COMPLETED_OUTCOME_EPOCH

ROSTFREI_NATS_URL=nats://127.0.0.1:4222 \
  ROSTFREI_APPLICATION=bike-rental-demo \
  ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES=67108864 \
  ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES=268435456 \
  ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES=524288 \
  ROSTFREI_API_TOKEN=local-development-token \
  ROSTFREI_INSPECTION_TOKEN=local-inspection-token \
  ROSTFREI_DISPATCH_TOKEN=local-dispatch-token \
  cargo run --locked -p bike-rental -- --tracer
```

Tracer listens at `http://127.0.0.1:1309`. Startup provisions local resources,
verifies the NATS topology, and seeds `city-fleet` with serviceable `bike-42` and
maintenance-required `bike-99` through actual `RegisterBicycle` commands. Stable operation
IDs replay their durable receipts on repeated startup, preserving later fleet changes.
Normal state persists across restarts; isolated
test state is recreated at startup when `--tracer` is enabled.

For CLI commands in another terminal, set the client environment too:

```sh
export ROSTFREI_TRACER_URL=http://127.0.0.1:1309
export ROSTFREI_API_TOKEN=local-development-token
```

Normal namespaces previously initialized by direct fixture injection need a fresh
application namespace; command seeding does not adopt receiptless fixture events.

Omit `--tracer` to start the same Normal command runtime without HTTP tokens,
endpoints or correlation observers. Both modes reuse one executable binding
definition. See the [shared host API and measured reduction](../../docs/nats-application-host.md).

Use [Tracer Studio](../../studio/README.md) to inspect and run behavioral tests.
For the **Infrastructure** view on this disposable local account, add
`ROSTFREI_INSPECT_BROKER=true` to the host environment. Leave
`ROSTFREI_DISPATCH_TOKEN` unset to keep production command dispatch disabled.
See [coverage and permissions](../../docs/nats-application-host.md#studio-operational-inventory).

## Domain model

There is one customer rental path: **reserve a category → collect a bicycle →
approve payment → start rental → return**. `CollectRentalPayment` is normally
triggered automatically after collection. `RentBicycle` is no longer an application
command, so HTTP, Studio and NATS cannot start a rental without approved payment.

| Concept | Example |
| --- | --- |
| Aggregates | `RentalFleetAggregate` owns fleet, reservations and collection holds; `RentalTariffAggregate` owns category pricing; `RentalPaymentAggregate` records payment attempts and separate rental/deposit amounts; `RepairJobAggregate` owns a workshop job. |
| Commands | `ConfigureRentalTariff`, `ReserveBicycleCategory`, `CollectReservedBicycle`, `CollectRentalPayment`, `RefundEligibleRentalDeposit`, `ReturnBicycle`, `RegisterBicycle`, `TransferBicycle`, and `OpenRepairJob` belong to the `BikeRental` bounded context. |
| Reservation | A customer books category capacity at one branch for a rental period; the confirmation does not assign a physical bicycle or collect payment. |
| Domain service | `BicycleTransfer` coordinates a transfer between two fleets. |
| Policy | `RentalEligibilityPolicy` evaluates bicycle condition and whether the rental lifecycle permits renting. |
| Lifecycle | A bicycle can be available, awaiting payment, rented, or retired. |
| Invariant | `FleetConsistency` demonstrates a rule over bicycle identities. |
| Value object | `RegistrationNumber` demonstrates local actions, policies, and invariants. |
| Query | `BicycleAvailabilityQuery` reads availability from fleet state. |

Command payloads carry all required fleet and bicycle IDs. `RegisterBicycle` takes
`fleet_id`, `bicycle_id`, and `condition`; it rejects an ID already in the fleet.
Its optional `category` records a category alongside the addition in one commit.
Existing uncategorized bicycles retain their original meaning and do not count
toward category reservations. Transfers preserve a bicycle's category.
`ReturnBicycle` accepts an optional `damage_report`; omitting it or passing `null`
keeps the ordinary return behavior. A supplied report records maintenance-required
condition alongside the return. Its payload schema remains 2; deploy readers
accepting the optional field before clients start sending it.
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

After `BicycleDamageReported` commits, the [workshop mapping](src/workshop.rs)
publishes `BicycleMaintenanceRequested`. A registered integration-command mapper
dispatches `OpenRepairJob` through NATS. Each damage event supplies a stable repair
job identity. Reopening that job with identical details is event-free; conflicting
details are rejected with `REPAIR_JOB_ALREADY_OPENED`. Rental and workshop are
separate aggregates in the `BikeRental` bounded context.

For the routing details, see [subjects and streams](https://elblabs.github.io/rostfrei/docs/messaging/subjects)
or the [same guide in this checkout](../../docs/messaging/subjects.mdx).

This development entry point explicitly provisions its NATS topology. The shared
host also offers a verify/connect-only startup for operator-provisioned resources.
It supervises command, domain-event, integration-event, observer and HTTP tasks,
and shuts down the owned group if any exits unexpectedly.

### Stable command subscription identities

Command consumer and durable names use `COMMAND_SUBSCRIPTION_GENERATION = 2`,
independently of `Command::SCHEMA_VERSION`. This preserves deployed names such as
`bike-rental--bike-rental--return-bicycle--v2` and
`bike-rental--test--bike-rental--return-bicycle--v2`. The suffix identifies the
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
current schemas; [`examples/event_evolution`](examples/event_evolution/main.rs)
shows typed private-event upcasts and a dual-reader/v1-writer compatibility release
([guide](../../docs/event-schema-evolution.md)). See [durable consumer versions](../../docs/messaging-upgrades.md#durable-consumer-versions).

### Updating an older demonstration

Use a fresh application namespace for this renamed example. Stop old writers and
finish or explicitly retire pending demo work before switching clients. The current
commands replace `add-bicycle`, `process-rental-payment` and `settle-rental-deposit`
with `register-bicycle`, `collect-rental-payment` and `refund-eligible-rental-deposit`;
`rent-bicycle` has no replacement shortcut. Old operation receipts remain in their
original namespace. Historical payment/collection events still replay with their
recorded outcomes; new writes use approval/decline and collected/cancelled facts.
Historical event readers remain required while those histories are retained.

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

With `ROSTFREI_INSPECTION_TOKEN` configured, Studio's **Correlations** workspace
can select the rental-fleet anchor (for example, `city-fleet`) or open a correlation
ID. It reads retained Normal domain and integration events directly from NATS;
source cutoffs describe each read. There is no projector to start. Integration
events keep their 30-day default expiry. Includes and payload-field matches are
described in [retained inspection](../../docs/continuous-observation.md#retained-correlation-inspection).

This event-subject format requires a fresh application namespace. Changing only
the completed-outcome epoch does not migrate older event subjects; see
[fresh deployments](../../docs/messaging/subjects.mdx#fresh-deployments).

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
events through the Test-only fixture capability. Normal startup uses `demo::seed_demo`
instead of fixture replay. There is no legacy event decoder; incompatible demo histories require a
fresh `ROSTFREI_APPLICATION` namespace or deletion of disposable NATS streams.

For example, renting `bike-42` once succeeds. A second rental with a new
idempotency key is rejected with `BICYCLE_UNAVAILABLE`. Inspect that recorded
outcome without submitting again. Reset restores the initial state.

### Advance reservations

`reserve-bicycle-category` schema 1 confirms a reservation immediately when the
branch has capacity and a configured category tariff. A reservation contains a
customer reference, category, fixed rental quote and RFC 3339 start/end timestamps.
The end is exclusive, so adjacent bookings can use
the same capacity. Capacity is checked at every overlapping boundary, rather
than by summing all bookings that touch the requested period.

Only serviceable bicycles in the requested category count. A bicycle held for
collection stays in category capacity while its reservation occupies a slot;
it cannot be hired or transferred during payment. Existing
rentals have no recorded due time, so they remain excluded until returned;
maintenance-required and retired bikes are excluded too. The fleet owns both
inventory and reservations, so competing confirmations use the same optimistic
concurrency boundary. Identical reservation details under the same reservation
ID are event-free; conflicting details are refused. Payment is due at collection,
and confirmation records no financial transaction.

Configure pricing with `configure-rental-tariff` schema 1. The category identifies
the company-wide tariff; monetary amounts are integer minor units:

```json
{
  "category": "city",
  "terms": { "currency": "EUR", "hourly_rate_minor": 500, "daily_cap_minor": 2500, "security_deposit_minor": 10000 }
}
```

Each started hour is billed, capped within each elapsed 24-hour block from the
reserved start. With this tariff, 1h10m costs €10, 8h costs €25, and 25h10m costs
€35. A cap never increases the hourly charge. Tariffs require a three-letter
uppercase currency, positive rates and a configured security-deposit amount
(zero explicitly waives the deposit). An unrepresentable rental total cannot
become a quote. A missing or incomplete tariff rejects a new booking with
`quote-unavailable`.

The confirmation stores `quote.currency`, `quote.rental_charge_minor` and
`quote.security_deposit_minor`. The deposit is separate from the rental charge:
the sample booking has €25 rent and a €100 refundable deposit due at collection.
Later tariff changes apply to new bookings; retrying an existing reservation
preserves both original amounts. The tariff is a read participant in the reservation
commit, so concurrent pricing changes use the framework's read guards.
Historical events without a quote or deposit still replay with that information
unknown; they are never silently repriced or treated as a zero deposit.

With the API running, execute the [reservation definition](tests/tracer/reserve-city-bike.json):

```sh
cargo rostfrei-dev test examples/bike-rental/tests/tracer/reserve-city-bike.json \
  --output reservation-evidence.json
```

It resets isolated Test to `reservable-branch` revision 3: the tariff above, one
serviceable city bike and one city bike awaiting repair. It confirms a booking
for `customer-alex` on 1 December 2026, 09:00–17:00 UTC with €25 rent and a €100 deposit. The
[NATS runtime suite](tests/nats_runtime_integration.rs) also checks that a retry
does not duplicate the booking and an overlapping request cannot consume the
unserviceable bike as spare capacity, and that tariff changes preserve earlier
quotes while affecting new bookings.

### Collection and sandbox payment

Reset isolated Test to `booked-city-bike` revision 1, then submit
`collect-reserved-bicycle` schema 1:

```json
{
  "fleet_id": "city-fleet",
  "reservation_id": "booking-42",
  "bicycle_id": "bike-42",
  "payment_id": "payment-42",
  "payment_method": "sandbox-approved"
}
```

The clerk selects a serviceable, available bicycle in the booked category.
Collection atomically records its `awaiting-payment` hold and a payment request
for the reservation's original rental charge and refundable deposit. An accepted
collection command means payment was requested; handover waits for approval.

A post-commit integration message triggers `collect-rental-payment`. The
deterministic sandbox approves `sandbox-approved`; `sandbox-declined` and unknown
references decline. It moves no real money. `RentalPaymentApproved` records the
provider reference in the same commit as `BicycleRented` and
`ReservedBicycleCollected`. `RentalPaymentDeclined` and `BicycleCollectionCancelled`
release the bicycle while retaining the confirmed booking. Use a new `payment_id` for a later
attempt; repeating an old attempt replays that attempt rather than charging again.

Payment identities cannot be reused with a different branch, reservation, bicycle
or payment method. A pending reservation cannot start a second attempt. Completed
processing is event-free on retry, and an old decline cannot release a newer
rental. The provider receives the payment identity as its idempotency key across
handler retries; this sandbox returns a deterministic result for it. It is not a
real payment-gateway adapter.

Run [the payment approval definition](tests/tracer/approve-collection-payment.json) with:

```sh
cargo rostfrei-dev test examples/bike-rental/tests/tracer/approve-collection-payment.json \
  --output collection-evidence.json
```

The definition starts from `pending-city-collection` revision 1: the bike is
held and payment requested. It invokes `CollectRentalPayment` and checks
approval, rental start and the linked integration event. Fixture replay does not
trigger the payment worker. Worker-generated commands are absent from this
version's Tracer capture, so the full automatic handoff has grouped fidelity and
cannot be represented by its strict behavior comparator. The
[NATS runtime tests](tests/nats_runtime_integration.rs) gate provider confirmation
to verify the hold, then independently inspect the authoritative operation
receipt, payment/rental state, retry protection and decline recovery.

### Returns and deposit settlement

Record a return with `return-bicycle` schema 2, including the actual RFC 3339
return time for a collected reservation:

```json
{
  "fleet_id": "city-fleet",
  "bicycle_id": "bike-42",
  "returned_at": "2026-12-01T17:00:00Z"
}
```

The physical return and its reservation/payment association commit together.
`reserved-bicycle-returned` becomes `rental-return-recorded`, which automatically
dispatches `refund-eligible-rental-deposit`. The bike is returned before the refund finishes.
A serviceable return with no damage report, at or after the booked start and no
later than the booked end, refunds the full agreed deposit to the original charge.
Returning exactly at the end is on time; equivalent timezone offsets compare as
instants. The rental charge stays paid, including after an early return.

A damaged or late return keeps the deposit held for later assessment. Damage also
blocks hire and opens the workshop job. Missing return times remain unknown and
keep the deposit held; historical events never invent a timestamp. `returned_at`
is an optional compatible addition to schema 2: deploy readers before clients
start sending it. Invalid timestamp strings fail payload decoding.

Settlement verifies the persisted return against the payment's original branch,
reservation and bicycle before contacting the provider. The sandbox confirms the
refund with `sandbox:deposit-refund:<payment-id>`; its idempotency namespace is
separate from charging. `security-deposit-refunded` records the currency, deposit
amount, original charge reference and refund reference only after approval.
Ineligible returns accept the conditional refund request without issuing a refund.
Zero deposits require no provider call. Completed refund retries are event-free
and do not contact the provider again. A declined refund leaves the deposit held;
an explicit later settlement attempt uses a new command operation identity.

Run the [refund definition](tests/tracer/refund-security-deposit.json) against the
running isolated host:

```sh
cargo rostfrei-dev test examples/bike-rental/tests/tracer/refund-security-deposit.json \
  --output refund-evidence.json
```

Its `returned-city-bike` revision 1 fixture establishes a paid €25 rental, €100
deposit and a satisfactory return at the booked deadline. The definition checks
the actual settlement command and refund event. The NATS runtime suite verifies
the complete automatic handoff, pending-provider state, retained deposits and
duplicate protection.

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
Run reserve-city-bike and explain the confirmed category, quote and deposit.
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
cargo xtask test -- cargo test --locked -p bike-rental -- --test-threads=1
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
integration-command reactions, damaged-return protection, reservation capacity,
and seven behavioral definitions. They fail if the NATS URL is missing or
unreachable, or the server version is unsupported.
