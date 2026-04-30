# ADR 0021: Slim value objects and ordinary DTOs

## Status

Accepted.

## Context

`ValueObject` previously combined a semantic domain tag with ownership,
attached actions, and inferred structural metadata. Action inputs and query
outputs were consequently promoted to value objects merely so their Rust
shapes could appear in action or query descriptors.

That blurred durable domain concepts with operation-specific data transfer
types. It also made descriptive input and output metadata look executable even
though Rust method signatures were already the authoritative contracts.

## Decision

A genuine value object declares only stable semantic identity:

```rust
#[derive(ValueObject)]
#[domain(id = "bicycle-condition", label = "Bicycle condition")]
pub enum BicycleCondition {
    Serviceable,
    MaintenanceRequired,
}
```

`ValueObject` has no owner or attached-action attributes. Its compiled-model
descriptor contains its ID and label, not an inferred field or variant shape.
The `value_objects` inventory in `domain_model!` contains semantic value objects
only.

Action inputs, query outputs, imported records, projections, and similar
operation-specific shapes are ordinary Rust structs and enums:

```rust
pub struct ImportRentalFleetInput {
    bicycles: Vec<ImportedBicycle>,
}

pub enum BicycleAvailability {
    Available,
    Unavailable,
}
```

They do not derive `ValueObject`, carry `#[domain(...)]` metadata, or appear in
the value-object inventory. Action and query methods retain their authored Rust
signatures, including these arbitrary DTO types. Their macros tag the operation
but do not infer or project input and output shapes. `DecisionOutcome` remains
the explicit semantic metadata contract for decision result vocabularies, with
its payload simplification specified by
[ADR 0022](0022-semantic-decision-outcomes-and-ordinary-payloads.md).

[ADR 0029](0029-trait-preserving-singular-domain-queries.md) subsequently
makes each query a singular ordinary trait implemented directly for its
aggregate root and removes query-group model registration altogether.

The typed filesystem treats semantic value-object declarations as leaf tags in
their owning domain module. Plain `input.rs` and `output.rs` files are valid
companions in action and query directories and do not require a Rostfrei
primary declaration.

Tools must not invent schemas or examples for opaque value objects or ordinary
DTOs. When the tracer catalog encounters a value-object-shaped field without an
explicit external schema, its payload template is JSON `null`.

## Consequences

This is a breaking source and model change. Applications remove `owner` and
`actions` from genuine value objects, remove `ValueObject` derives and domain
field annotations from DTOs, and remove those DTOs from `domain_model!`.

The compiled model becomes intentionally smaller and no longer claims to be a
Rust DTO schema. Domain concepts retain stable IDs and labels, while operation
payload evolution remains governed by ordinary Rust types and boundary-specific
serialization or schema contracts.
