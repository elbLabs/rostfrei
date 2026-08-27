---
title: Domain Command
kind: reference
---

# Domain Command

## Definition

A **Domain Command** is a structured application request routed to an Aggregate
or Domain Service command handler.

```rust
#[derive(DomainCommand)]
#[domain(
    id = "rent-bicycle",
    label = "Rent bicycle",
    owner = RentalFleetAggregate,
    rejection = BicycleUnavailable,
    json,
    runtime,
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

The opt-in `runtime` flag generates the command's runtime definition from its
modeled owner, ID, and schema version. Use it for commands that will be bound to
an executable Aggregate. Metadata-only commands omit it and do not require a
runtime Aggregate or command handler.

Commands must be inventoried explicitly:

```rust
domain_model! {
    // Other inventories omitted.
    commands: [RentBicycle],
}
```

The compiled model projects these descriptors in top-level `domainCommands`.
Commands are not Action inputs. A command handler translates command fields into
one or more scalar, Value Object, or aggregate-owned Domain Identity Action
inputs and coordinates those Actions.
This keeps message metadata, idempotency, routing, and wire concerns outside the
domain behavior contract.

Explicit model construction rejects a duplicate `DomainCommandId`; it does not
require a command to map one-to-one to an Action.
