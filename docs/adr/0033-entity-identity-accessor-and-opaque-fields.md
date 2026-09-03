# ADR 0033: Entity identity accessor and opaque custom fields

## Status

Accepted.

## Context

Entity derives previously selected one field with `#[domain(identity)]`, while
`EntityDefinition::Identity` separately named the identity type. Identity and
Value Object field tags were also repeated at every Command, Event, Error, and
Entity use site to produce structural metadata.

The tags coupled Rust storage layout to semantic identity and made custom types
look structurally modeled even when their wire behavior was governed by normal
serialization.

## Decision

`EntityDefinition` is the sole authority for obtaining an entity identity:

```rust
impl EntityDefinition for Bicycle {
    type Owner = RentalFleetAggregate;
    type Identity = BicycleId;

    fn identity(&self) -> &Self::Identity {
        &self.bicycle_id
    }
}
```

Entity fields have no identity marker. The accessor may return a stored field,
delegate to embedded state, or otherwise follow the entity's ordinary Rust
representation without procedural-macro field selection.

The `identity` and `value_object` field-role attributes are removed from all
use sites, including Entities, Commands, Events, and Domain Errors. A custom
Rust field type without another supported explicit role is projected as
`opaque`. This is an honest statement that Rostfrei does not own that type's
shape.

Entity metadata retains the Entity-scoped `DomainIdentityId`, discovered from
`EntityDefinition::Identity`, but no longer exposes an identity field name.
Domain identity inventory continues to be discovered from registered Entities.
Tracer payload templates render opaque values as JSON `null`.

Typed filesystem fixtures include the identity accessor in each valid
`EntityDefinition`; source structure does not require or recognize removed
field annotations.

## Consequences

This is a breaking source and descriptor-shape change. Every Entity definition
adds `identity(&self)`, and applications remove identity and Value Object field
tags. Consumers stop reading `entity.identity.field` and treat custom fields as
opaque unless a surviving explicit role applies.

Runtime serialization, event bytes, replay, command handling, and rejection
encoding remain governed by their existing Rust/Serde contracts and are not
changed by descriptor opacity.
