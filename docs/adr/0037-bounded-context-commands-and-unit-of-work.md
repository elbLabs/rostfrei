# ADR 0037: Bounded-context commands and command unit of work

## Status

Accepted. Supersedes the aggregate-command execution relationship in
[ADR 0026](0026-handler-linked-commands.md) and the aggregate-addressed command
HTTP route in [ADR 0019](0019-registered-queries-and-standard-http.md).

## Context

Aggregate-addressed commands require one aggregate type and aggregate ID in the
transport envelope. That is convenient for one-aggregate decisions, but it makes
one participant artificially primary for use cases that coordinate several
aggregates. It also splits a command's business identity between its payload and
transport routing metadata.

Rostfrei already supports atomic event transactions, optimistic stream versions,
and transaction read guards. The missing abstraction is an application-layer
unit of work that can load and stage every aggregate participating in one
bounded-context command.

## Decision

A command belongs to a bounded context rather than an aggregate:

```rust
#[derive(Command)]
#[domain(
    context = BikeRental,
    id = "transfer-bicycle",
    label = "Transfer bicycle",
    schema_version = 2
)]
pub struct TransferBicycle {
    pub bicycle_id: BicycleId,
    pub from_fleet_id: FleetId,
    pub to_fleet_id: FleetId,
}
```

Every aggregate identity required by a use case is part of the command payload.
`CommandRequest`, `DynamicCommandRequest`, `RoutedCommand`, command fingerprints,
registry identities, and HTTP command routes contain no aggregate routing fields.
The standard HTTP route is:

```text
POST /contexts/{context}/commands/{command}/schemas/{schema_version}
```

A command is handled by one application-layer object:

```rust
#[async_trait]
impl CommandHandler<TransferBicycle> for TransferBicycleHandler {
    type Rejection = BicycleTransferRejected;

    async fn handle(
        &self,
        command: &TransferBicycle,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        // Load aggregates and invoke domain behavior. Successful loads are
        // tracked automatically for an accepted decision.
    }
}
```

`CommandExecution` loads typed aggregates through the codec registered for that
aggregate type, falling back to the standard JSON event codec, records their base
versions, and automatically enlists every successful load. `CommandExecutor::with_codec::<A, _>`
and `CommandProcessor::register_codec::<A, _>` configure custom DTO, legacy,
upcasting, Protobuf, or other representations without forcing heterogeneous
participants through one codec. A loaded aggregate with events is a writer. A loaded aggregate without
events is a read guard when another participant writes. Failed loads enlist
nothing, and a loaded aggregate that escapes its command-handling attempt fails
closed. Mutation uses the direct `aggregate_mut()` API. Tracking belongs to the aggregate instance
returned by `load`, not to arbitrary aggregate instances manually constructed by application code.
When the loaded handle leaves the handler, its load lease verifies that the tracked instance is
attached before the unit of work can commit. A handle that finishes with a replacement or swapped
instance fails closed, as do escaped and forgotten handles. Temporarily constructed aggregates that
are discarded by application code do not become participants merely by being moved through a loaded
handle. Accepted and rejected decisions both validate every load lease; a rejected
command discards validated journals and persists nothing.

For automatically tracked aggregates, each raised event opens its journal entry before the event
is applied and completes it after synchronous encoding. Returned encoding failures are
reported when the accepted unit of work finishes; a caught encoding panic leaves
an incomplete journal and also fails closed before persistence.

Every event-producing command is committed as one `EventTransaction`, whether
it has one participant or many. There is no distinguished primary participant;
the first participant may be a read guard, while at least one participant must
write. Conflict retries recreate the complete unit of work and rerun the complete
handler.

The aggregate derive macro projects each aggregate's bounded-context identity into
the runtime `Aggregate` implementation. `CommandProcessor` is constructed for one
bounded context, rejects registrations and incoming commands for another context,
and places that trusted context in `CommandExecutionMetadata`. Every
`CommandExecution::load<A>` verifies `A::BOUNDED_CONTEXT` before accessing the
store. Direct executor callers must bind the context with
`CommandExecutionMetadata::with_bounded_context`; metadata without a context may
still be used by non-command infrastructure but cannot load command participants.

Transaction receipts are addressed by `(bounded context, OperationId)`. The context
is persisted in `EventTransaction` and `TransactionReceipt`, participates in exact
replay, and allows even a shared in-memory store to maintain independent operation
namespaces. NATS additionally verifies that the transaction context matches its
bounded-context-scoped event-store configuration. Exact replay validates the
fingerprint, correlation, causation, participant identities, and event
provenance before returning the original events.

`EventStore::append` remains a low-level, stream-scoped API for fixtures,
imports, and infrastructure operations. Its operation identity is intentionally
separate from command transaction identity; callers must not assume that one
namespace of operation IDs is globally arbitrated across low-level appends and
command transactions. `EventStore::append_transaction` and the context-aware
receipt lookup are mandatory capabilities of every `EventStore` implementation
used by command execution. Implementations must resolve
`load_transaction_receipt_in_context` in a context-scoped namespace rather than
implementing it by looking up a globally keyed operation and filtering afterward.
The operation-only lookup and contextless transaction values remain available
solely for legacy migration and low-level store tooling; `CommandExecutor` never
uses them.

An accepted command with no emitted events has no durable event-store receipt.
Its `CommandReceipt::NoEvents` result may therefore rerun on retry. Domain
rejections are likewise not persisted by the event store; durable command
transport may retain their command response separately.

Command registration is keyed by bounded context, command name, and schema
version. Aggregate inventory used for stream discovery is registered explicitly
and is not inferred from command handlers.

Tracer command routes, catalog entries, operations, and command observations are
bounded-context scoped. Domain events, fixtures, predicted events, and touched
stream participants retain aggregate type and ID. The incompatible Tracer
behavioral-test, message-series-definition, and observed-message-series shapes
are published as schema version 2.

This is an intentional breaking migration, not a dual-version runtime. The
checked-in v1 JSON Schema files remain unchanged only as offline references for
identifying and converting stored documents. Tracer does not advertise or serve
those schemas, its parser and test repository do not accept v1 behavioral-test
documents, and it does not translate v1 documents during reads or test runs.
Likewise, the removed aggregate-addressed Tracer command input, Simulation,
Test, and Dispatch routes have no executable compatibility aliases; requests to
the old paths receive `404 Not Found`. Catalog v1 is the version of the discovery
document and does not imply runtime support for Tracer schema-v1 documents.

NATS event-store streams contain both aggregate events and transaction bookkeeping. Correlation
observers subscribe to the store's aggregate-event subject filter rather than the broader domain
family, so receipts and read guards cannot be misclassified as observable domain events. Decode
failures on actual aggregate-event subjects remain fail-closed.

## Migration

Before deploying this change:

1. Drain or isolate pending commands that use the old aggregate-addressed wire
   envelope; the new runtime does not translate them. Before switching runtimes,
   also let the old command transport's retry/idempotency window expire or rotate
   its operation-ID namespace. Receipts written with the legacy
   primary-aggregate-derived subject cannot be found by the new command replay
   entry point from an `OperationId` alone and are not rewritten automatically.
   Do not retry pre-upgrade operation IDs against the new runtime. Existing legacy
   receipts remain readable only while validating history through their known
   primary aggregate stream; operation-only lookup remains a migration/tooling API.
2. Make every required aggregate ID a field in the command payload, declare the
   command's bounded context, and publish/register a new command payload schema
   version when that payload contract changed.
3. Convert persisted behavioral-test documents to document `schemaVersion: 2`.
   In behavioral tests and standalone expected message-series definitions, replace
   every command node's `aggregate: { "type": "{context}/{aggregate}", "id": "..." }`
   with `context: "{context}"`, move the former aggregate ID into the appropriate
   command payload field or fields, and set the node's `schemaVersion` to the
   registered command payload schema version. Standalone message-series documents
   do not have a top-level `schemaVersion`; adding one makes them invalid.
4. Regenerate stored observed message series using the v2 shape, also without a
   top-level `schemaVersion`. Command observations now carry `context` instead of
   `aggregate`; domain-event aggregate identity and operation participant identity
   remain unchanged.
5. Update Tracer clients to start from Catalog v1 and follow its advertised v2
   schema and bounded-context command links. Do not rewrite old URLs mechanically:
   construct requests from the discovered templates and self-contained payload.
6. Keep v1 schema artifacts only for offline validation of source material while
   converting it. Do not leave v1 JSON in a runtime test repository, because
   repository loading fails closed rather than upgrading it.

## Consequences

Command handlers are application services. Single-aggregate and
multi-aggregate commands use the same handler, registration, result, simulation,
and persistence abstractions. There is no `TransactionalCommandHandler`, no
execution-mode hierarchy, and no command fan-out to several handlers.

Simple commands must include their aggregate identity in their payload and load
the aggregate explicitly. This is additional ceremony, but it keeps complete
intent in one canonical payload and removes hidden aggregate routing from
transport.

The command wire contract, HTTP routes, command fingerprints, handler API,
registry identity, and command schemas are breaking changes. Pending legacy
command messages must be drained or isolated during deployment. NATS continues
to read historical primary-addressed transaction receipts while all new receipts
use operation-only subjects.

A saga or process manager remains a separate pattern for workflows whose
participants cannot share one bounded-context event-store transaction. Local
multi-aggregate commands should use the unit of work rather than introduce
intermediate reservation and compensation states without a business need.
