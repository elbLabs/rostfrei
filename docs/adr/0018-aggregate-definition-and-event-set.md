# ADR 0018: Separate aggregate identity, definition, and event set

## Status

Accepted.

## Context

The original `Aggregate` derive accepted context, root, action, decision,
invariant, and event lists in one `#[domain(...)]` attribute. That made the
derive the place where applications manually repeated relationships that Rust
code already expresses, or that Rostfrei cannot prove. In particular, an
attached action can claim that it raises an event without the action
implementation necessarily doing so.

Aggregate runtime execution still needs three concrete type relationships:
the bounded context, aggregate root, and closed event representation. Those
relationships are executable Rust contracts and should remain compiler
checked.

## Decision

`#[derive(Aggregate)]` declares only the aggregate's semantic identity:

```rust
#[derive(Aggregate)]
#[domain(id = "rental-fleet", label = "Rental fleet")]
pub struct RentalFleetAggregate;
```

An ordinary `AggregateDefinition` implementation supplies the executable type
relationships:

```rust
impl AggregateDefinition for RentalFleetAggregate {
    type Context = BikeRental;
    type Root = RentalFleet;
    type Event = RentalFleetEvent;
}
```

The event representation is a separate, explicit enum:

```rust
#[derive(AggregateEvents)]
pub enum RentalFleetEvent {
    BicycleAdded(BicycleAdded),
    BicycleRented(BicycleRented),
}
```

Each variant is a real runtime conversion and dispatch relationship, rather
than descriptive attachment metadata.

The typed application structure requires an aggregate directory to contain:

```text
<aggregate>/
├── aggregate.rs   # one Aggregate and one AggregateDefinition impl
└── event_set.rs   # one AggregateEvents enum
```

`mod.rs` continues to contain composition only.

Actions, decisions, and invariants are not attached through the aggregate
derive. Their contracts and implementations remain available as normal Rust
code, but aggregate-level compiled-model inventory and automatic invariant
fanout are intentionally absent until Rostfrei has relationships it can derive
or validate without manual lists.

## Consequences

This is a breaking source change. Applications must remove `context`, `root`,
`events`, `actions`, `decisions`, and `invariants` from aggregate attributes,
add an `AggregateDefinition` implementation, and introduce an aggregate event
set enum.

Runtime code gains a single compiler-checked event type, and aggregate
definition errors are reported through ordinary associated-type constraints.
The filesystem checker can enforce the convention deterministically without
attempting Rust name resolution.

`AggregateType` now carries descriptor identity only. Generic code that
previously accessed `<A as AggregateType>::Context` or
`<A as AggregateType>::Root` must use `AggregateDefinition` instead.

Event ownership is now expressed through the aggregate type parameter on
`DomainEventType`. Code that accesses owned event metadata must name that
aggregate explicitly, for example:

```rust
<BicycleAdded as DomainEventType<RentalFleetAggregate>>::DESCRIPTOR
```

Aggregate event sets generate event outputs only for Aggregate action
contracts. Domain Service actions no longer use aggregate-owned Domain Events
as modeled outputs because their signatures do not identify the owning
Aggregate, and Rostfrei has no runtime consumer for that descriptive
relationship.

Aggregate-derived application types contribute no action, decision, or
invariant attachments to compiled domain models. The low-level `AggregateType`
inventory constants remain available for manual implementations and focused
framework tests, but the derive no longer populates them. Tooling must not
infer those relationships from the old lists. A later decision may derive
relationships from stronger executable contracts or the typed project
structure, but this ADR does not choose that mechanism.
