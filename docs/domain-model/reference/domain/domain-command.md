---
title: Domain Command
kind: reference
---

# Domain Command

## Definition

A **Domain Command** is the structured request accepted by an Action.

```rust
#[derive(DomainCommand)]
#[domain(
    id = "rent-bicycle",
    label = "Rent bicycle",
    owner = RentalFleetAggregate,
    rejection = BicycleUnavailable,
    json,
)]
struct RentBicycle {
    #[domain(identity)]
    bicycle_id: BicycleId,
}
```

`DomainCommand` supports non-generic structs and the standard scalar, identity,
Value Object, aggregate-reference, `Vec`, and `Option` field machinery. Fields
may also select a [Custom scalar](custom-scalar.md) with
`#[domain(scalar = Provider)]`. Contained Entities are invalid. Only Aggregates
and Domain Services can own commands; Entity and Value Object owners are
rejected by the generated trait bounds.

A command is first-class metadata. `DomainCommandType` exposes its associated
`Owner`, `LOCAL_ID`, and `DESCRIPTOR`. The descriptor contains a stable
owner-scoped `DomainCommandId`, label, and fields.

`schema_version = N` changes the default schema version of `1`. An
aggregate-owned command may declare its modeled rejection with
`rejection = ErrorType`. The opt-in `json` flag generates a conventional JSON
decoder used by `ControlPlaneBuilder::register_json`; named command payloads are
objects with no unknown fields, tuple payloads are exact-length arrays, and unit
commands accept `null` or an empty object. Applications can instead register an
explicit command wire codec.

Commands must be inventoried explicitly:

```rust
domain_model! {
    // Other inventories omitted.
    commands: [RentBicycle],
}
```

The compiled model projects these descriptors in top-level `domainCommands`.
An action input contains only `{ "kind": "domainCommand", "id": ... }`; fields
remain on the command inventory item. The link is inferred from the Rust input
type. There is no action command attribute.

The command owner must exactly equal the action owner. Scalars and Value
Objects remain valid action inputs for compatibility, while commands cannot be
used by Entity or Value Object actions. Explicit model construction rejects a
duplicate `DomainCommandId`; it does not require commands to be referenced or
limit a command to one action.
