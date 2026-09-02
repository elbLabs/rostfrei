# ADR 0028: Trait-preserving singular domain actions

## Status

Accepted.

## Context

Plural `domain_actions` contracts grouped multiple methods and required an
owner kind. Generated adapter traits and action-group model plumbing obscured
the ordinary Rust behavior that applications actually call. Ownership metadata
could also drift from the implementor and typed filesystem location.

## Decision

Each action is one ordinary trait with one authored method:

```rust
#[domain_action(id = "rent-bicycle", label = "Rent bicycle")]
pub trait RentBicycleAction {
    fn rent_bicycle(&mut self, bicycle_id: BicycleId)
        -> Result<(), BicycleUnavailable>;
}
```

The attribute preserves the trait and adds `LOCAL_ID`, `LABEL`, and
`DESCRIPTOR` associated constants. It does not accept an aggregate, entity,
domain-service, or value-object owner kind.

Behavior is a direct trait implementation on its real receiver:

```rust
impl RentBicycleAction for AggregateInstance<RentalFleetAggregate> {
    fn rent_bicycle(&mut self, bicycle_id: BicycleId)
        -> Result<(), BicycleUnavailable>
    {
        // validate, raise, and return through ordinary Rust
    }
}
```

Entity actions are implemented directly for the entity. Call sites retain
normal method syntax such as `aggregate.rent_bicycle(id)` and
`bicycle.mark_rented()`.

Action metadata is owner-independent. Plural groups, owner marker traits,
attachments, action extensions, and compiled-model action projection are
removed. The typed filesystem infers conceptual ownership from nesting: an
`action.rs` file contains exactly one singular action trait and sits below its
aggregate, entity, or demonstrated domain-service parent.

Action-focused tests name the concrete relationship explicitly:

```rust
#[domain_action_test(<AggregateInstance<RentalFleetAggregate>
    as RentBicycleAction>::DESCRIPTOR)]
fn rents_an_available_bicycle() {}
```

## Consequences

This is a breaking source and model change. Applications replace plural action
macros and generated group traits with singular annotated traits and direct
implementations. They remove action owner arguments, action-group adapters,
and `action_extensions`.

Action behavior is now idiomatic, discoverable Rust. Metadata describes the
action itself without claiming an owner relationship or maintaining a second
model inventory.
