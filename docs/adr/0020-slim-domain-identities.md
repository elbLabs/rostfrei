# ADR 0020: Slim domain identities

## Status

Accepted.

## Context

`DomainIdentity` originally required `owner` metadata and inferred a scalar
representation from the newtype field, with an optional semantic-scalar
provider. The owner relationship was already expressed by
`EntityDefinition::Identity`, so the identity declaration repeated information
and could drift from the entity definition.

Treating every identity as a modeled scalar also constrained otherwise useful
opaque newtypes. In particular, a domain identity may wrap a UUID or another
validated application type without that representation being meaningful domain
metadata.

## Decision

`DomainIdentity` is a marker derive for a non-generic struct or enum. It
accepts no owner or scalar attributes and does not prescribe the identity's
internal Rust shape:

```rust
#[derive(DomainIdentity)]
pub struct BicycleId(uuid::Uuid);
```

The identity is discovered and bound through its entity definition:

```rust
impl EntityDefinition for Bicycle {
    type Owner = RentalFleetAggregate;
    type Identity = BicycleId;
}
```

`domain_model!` therefore has no separate `identities` inventory. Adding an
entity supplies its identity relationship; applications do not independently
register identity descriptors.

Identity metadata does not claim a primitive scalar or semantic-scalar
representation. Tools that need an example payload must not guess from the
Rust type. The tracer catalog uses JSON `null` as the honest identity payload
template until an application supplies an explicit schema or example through
a separate contract.

The typed filesystem keeps `identity.rs` as the conventional role file. It
must contain exactly one `DomainIdentity` declaration in that location, but the
marker itself needs no domain attribute.

## Consequences

This is a breaking source and model-composition change. Applications remove
`owner` and `scalar` from `DomainIdentity`, remove the `identities` list from
`domain_model!`, and retain the identity association in `EntityDefinition`.

Ownership has one compiler-checked source of truth. Identity types may wrap
UUIDs, use composite structs, or represent imported identifier variants
without pretending they are primitive domain scalars. Generated catalogs lose
speculative identity examples and use `null`, making the absence of
representation metadata explicit.
