# ADR 0029: Trait-preserving singular domain queries

## Status

Accepted.

## Context

Plural `domain_queries` blocks attached a group of query methods to an
aggregate type and projected their descriptors through a separate
`query_groups` model inventory. The actual behavior reads an aggregate root,
but the declaration, generated group, model registration, and call path all
repeated that relationship.

## Decision

Each query is one preserved ordinary trait:

```rust
#[domain_query(id = "bicycle-availability", label = "Bicycle availability")]
pub trait BicycleAvailabilityQuery {
    fn bicycle_availability(&self, bicycle_id: &BicycleId)
        -> Option<BicycleAvailability>;
}
```

The attribute adds `LOCAL_ID`, `LABEL`, and `DESCRIPTOR` associated constants.
The query is implemented directly for the root selected by the enclosing
aggregate definition:

```rust
impl BicycleAvailabilityQuery for RentalFleet {
    fn bicycle_availability(&self, bicycle_id: &BicycleId)
        -> Option<BicycleAvailability>
    {
        // read-only domain behavior
    }
}
```

Callers use ordinary method syntax such as
`fleet.state().bicycle_availability(&id)`. There is no generated query group,
aggregate owner argument, or free-function root parameter.

Queries are not projected into the compiled domain model. `domain_model!` has
no `query_groups` inventory and the model's queries collection remains empty.
Input and output DTOs continue to be ordinary Rust types under ADR 0021.

The typed filesystem keeps query directories directly beneath aggregates. A
`query.rs` file contains the singular query trait and `execute.rs` contains
exactly one direct, unqualified, unaliased implementation for the direct
`AggregateDefinition::Root` type. Qualified or aliased traits and roots, glob
imports, duplicate implementations, and missing execution files are rejected.
Private helper functions remain valid in `execute.rs`.

## Consequences

This is a breaking source and model change. Applications replace plural query
blocks with singular traits, implement them for the aggregate root, remove
generated group reexports and `query_groups`, and call root methods directly.

Query behavior and its receiver are now explicit Rust. The structure checker
links that receiver to `AggregateDefinition::Root` without attempting general
name resolution or allowing aliases to bypass the convention.
