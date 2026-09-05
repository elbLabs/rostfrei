# Bike rental example

This public example models a bicycle rental fleet. It demonstrates rostfrei's
compiled domain metadata without depending on a production application:

- `RentalFleetAggregate` owns the fleet and its bicycles;
- `RentBicycle`, `ReturnBicycle`, and `AddBicycle` are public aggregate commands;
- `RetireBicycleAction` demonstrates one logical transition from either available or rented;
- `RentalEligibilityPolicy` composes bicycle condition policy with the rental lifecycle;
- bicycle-added, rented, returned, and retired events describe successful domain transitions;
- fleet import is a privileged snapshot-restoration boundary that accepts existing lifecycle
  states but validates aggregate invariants before replacement;
- unavailable and not-rented errors describe command rejections;
- `FleetConsistency` rejects imports containing duplicate bicycle identities;
- `RegistrationNumber` is an isolated demonstration of Value Object-local actions, invariants,
  and policies; and
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

- `bike-rental.test.>` is recreated and its default `demo-fleet` MessageSeries
  fixture is applied on startup and by `POST /test-scenario/reset`;
- normal `bike-rental` subjects such as `bike-rental.command.>` persist across
  restarts and are never affected by test reset.

Upgrades preserve the exact `seed-city-fleet` demo history written by the
earlier behavioral-test runtime. Fresh namespaces use the canonical
`demo-fleet` MessageSeries fixture; any other conflicting seed history still
fails startup.

Each scope has separate command, command-response, integration-event,
quarantine, and authoritative domain-event streams, plus separate durables.
Test resources use the `BIKE_RENTAL__TEST` stream prefix. Test reset stops its
workers, recreates that complete topology, replays the selected fixture's
domain-event series through the shared MessageSeries engine, and restarts the
workers without touching normal Dispatch resources.
A failed reset leaves Test, Simulate, instances, and dynamic inputs unavailable
until a later reset succeeds rather than exposing partially rebuilt state.

Both initially contain `city-fleet`, serviceable `bike-42`, and
maintenance-required `bike-99`. The local example explicitly provisions these
streams and exposes trace payloads for demonstration. Production deployments
should provision infrastructure separately and use distinct NATS credentials or
accounts for Test and Dispatch.

## Agent-first Tracer workflow

The repository-local OpenCode skill at
`.opencode/skills/rostfrei-tracer/SKILL.md` discovers Tracer capabilities from
Catalog v1. Run OpenCode from the repository root so it loads that skill:

```sh
export ROSTFREI_TRACER_URL=http://127.0.0.1:1309
export ROSTFREI_API_TOKEN=local-development-token
export ROSTFREI_DISPATCH_TOKEN=local-dispatch-token
opencode
```

`ROSTFREI_TRACER_URL` defaults to `http://127.0.0.1:1309`. The control token is
used for discovery, Preview, isolated Test publication, behavioral tests, reset,
and inspection of those operations. The separate dispatch token is used only
for production dispatch and inspection of production operations. The skill is
instructed not to print or persist either token.

Ask in domain language rather than assembling HTTP routes. For example:

```text
List the available Rostfrei behavioral tests and summarize their intent.

Read the rent-available-bicycle behavioral test and explain it as Given, When,
Then without running it.

Run the rent-available-bicycle behavioral test. Explain the result and render
the observed causal message series as Mermaid.

Preview renting bike-42 from city-fleet and show the causal Mermaid flow.

Publish a Test rental of bike-42 from city-fleet, then explain the result and
show its causal Mermaid flow.

Return the currently rented bicycle in isolated Test state and visualize what
happened.

Preview adding a bicycle to city-fleet and visualize the predicted messages.
```

Preview is read-only and never appends. Test publication uses the normal command
pipeline but appends only to isolated Test state. Before a Test publication or a
behavioral-test run, the agent presents the exact action and asks for the
required confirmation; a behavioral-test run resets the complete isolated test
scenario to its selected fixture before the subject command executes.

The skill starts from Catalog v1 and follows its advertised links for commands,
schema versions, isolated test instances, dynamic test inputs, behavioral-test
definitions, fixture MessageSeries, actions, reset, operations, and finite
message series. It does not embed bike-rental routes or silently select among
multiple schema versions.
Values discovered through test links are treated only as isolated Test data and
are never assumed to exist in production.

For production, make the target and intent explicit:

```text
Dispatch RentBicycle schema version 1 to production aggregate city-fleet with
payload bicycle_id bike-42 and idempotency key prod-rental-2026-09-02-01. Show
the exact request for confirmation before publishing, then explain the result
and render its causal Mermaid flow.
```

The agent requires independently supplied production aggregate and payload
values, uses only `ROSTFREI_DISPATCH_TOKEN`, warns that production and downstream
integrations may change, and asks for explicit confirmation. It never
automatically retries an ambiguous publication or an `indeterminate` operation.

Every operation snapshot distinguishes execution lifecycle from business-event
evidence. `operationEventsHref` links to the Tracer lifecycle SSE stream, while
`correlationEventsHref` links to the correlated business-message SSE stream.
The `events` descriptor identifies the relevant evidence: Simulation reports
`kind: predicted` and links to operation events; Test and Dispatch report
`kind: observed` and link to correlation events. Consequently, `predictedEvents`
is serialized only for accepted Simulation results and is omitted from
transported results.

After a command, the skill can follow the operation's `messageSeriesHref` and
wait for terminal status plus a bounded idle settling window. Unlike the live
SSE links, this produces a finite capture containing the canonical messages and
command outcomes. Rejections, no-event decisions, or an unsettled timeout may
omit some message categories or produce a partial series. Mermaid edges are
drawn only from an explicit `causationId` to its matching `messageId`; array
position and timestamps never imply causality. Preview or
incomplete/conflicting identities are reported with grouped fidelity, while
complete consistent transported identities are exact.

Behavioral tests are canonical JSON documents in `tests/tracer`. They select a
fixture from `fixtures/` and describe one expected causal graph. Fixtures are
MessageSeries documents containing only the domain events replayed into isolated
history. The root command in `expected.graphs[0]` is then published
through the isolated Test NATS WorkQueue. The server constructs the observed
command, durable response, domain-event, and integration-event series and
performs the comparison. Expected JSON objects use subset semantics, while
scalar values and array lengths/order must match exactly. The filesystem remains
the source of truth, and the skill can list, read, validate, and run both
persisted and inline JSON definitions through advertised Catalog links.

Run the dispatch-isolation check and all three behavioral definitions against
a real NATS Server 2.12.1 or newer. These tests are deliberately ignored during
normal test runs; the explicit command is:

```sh
ROSTFREI_NATS_URL=nats://127.0.0.1:4222 \
  ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES=67108864 \
  ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES=268435456 \
  ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES=524288 \
  cargo test --locked -p bike-rental \
  --test nats_runtime_integration -- --ignored --test-threads=1
```

The opt-in run fails rather than skips when `ROSTFREI_NATS_URL` is missing,
empty, unreachable, or points to an unsupported server version.

The three Tracer actions have distinct semantics:

- `simulate` loads the current test NATS history but never appends;
- `test` publishes through the normal command pipeline and appends only to the
  isolated test NATS history;
- `dispatch` uses the identical pipeline against the canonical unsuffixed
  application namespace.

Both transported modes wait for command PubAck and a durable accepted or
rejected response. Accepted rentals then flow through a durable post-commit
domain-event handler, publish the correlated `bicycle-rental-started`
integration event, and consume it through the normal integration-event durable.
An `Idempotency-Key` is required for Test and Dispatch. The authenticated
`202 Accepted` submission response echoes it as `idempotencyKey` alongside the
resolved `operationId`, but retained operation resources do not store or return
the raw key. Test and Dispatch derive a mode-namespaced operation identity from
the key and command address, so clients must follow the response `Location`
header rather than assume the key is the operation ID. Idempotency keys should
not contain secrets.

If an error occurs after PubAck but before a valid durable response is observed,
the operation is `indeterminate` and preserves its command message identity
instead of claiming that the business command failed.

Running Test for `bike-42` twice accepts and appends the first command, then
replays that new state and rejects the second with `BICYCLE_UNAVAILABLE`.
Simulate subsequently observes the same rejection without changing history.
Reset returns the test stream to the default `demo-fleet` MessageSeries fixture
and does not touch Dispatch state.

`ReturnBicycle` advertises currently rented bicycles as runtime input choices and
makes the selected bicycle available again. `AddBicycle` has no user-supplied
payload. The aggregate assigns the next unused deterministic UUID and adds the
bicycle as available and serviceable. All three commands use their generated
JSON payload contracts; Tracer has no command-specific wire codecs.

Reusing an `Idempotency-Key` returns the retained operation only for the exact
same request, echoes the same key and resolved operation identity in the new
submission response, and returns `409 Conflict` for different content.
Each Test reset rotates the Test scenario generation, so a key reused afterward
cannot receive delayed correlation events from the previous scenario.

The route uses the context-qualified aggregate identity
`bike-rental/rental-fleet`. `RentBicycle`, its rejection, and aggregate events
use generated JSON contracts rather than handwritten bike-rental codecs.

Operation resources, traces, and correlation feeds remain count- and
byte-bounded in memory even though domain events are durable in NATS. Operation
payload retention has a 64 MiB aggregate budget. Raw correlation evidence has a
separate 64 MiB total budget split evenly between control modes and production
dispatch, so Preview or Test captures cannot consume dispatch evidence capacity.
Payloads that cannot fit a record's share are omitted rather than retained
without bound. Concurrent admission is bounded, and operation retention is
pressure-based rather than durable or time-based. This server is therefore a
local development example, not a production audit system.
