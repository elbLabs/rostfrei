---
title: Query
kind: reference
---

# Query

## Definition

A **Query** is a public, read-only projection owned by one [Aggregate](aggregate.md).

## Model Shape

```rust
#[domain_queries(group = BicycleAvailabilityQueries)]
impl RentalFleetAggregate {
    #[query(id = "bicycle-availability", label = "Bicycle availability")]
    pub fn bicycle_availability(
        root: &RentalFleet,
        input: &BicycleId,
    ) -> Option<BicycleAvailability> {
        let bicycle = root.bicycle(input)?;
        todo!()
    }
}
```

This query returns `Some(Available)` when the bicycle is serviceable and not
currently rented, `Some(Unavailable)` when it cannot be rented, and `None` when
the identity is unknown to the fleet.

The first parameter is exactly `root: &ExactRoot`. A query may have one additional `input: &T` parameter. Both references are immutable and have no explicit lifetime. The borrow is syntax only; the compiled input descriptor describes `T`.

Inputs may be canonical scalars, Value Objects, or Domain Identities owned by an
Entity in the queried Aggregate. Outputs are required and owned. They may be
canonical scalars, Value Objects, same-Aggregate Domain Identities, or
recursively nested `Option` and `Vec` values. A
[Custom scalar](custom-scalar.md) may appear inside an annotated Value Object or
Domain Identity, but is not yet supported as a raw Query input or output.

Queries are public associated functions without a receiver. Async, unsafe,
extern, variadic, generic, and where-qualified functions are unsupported.
Commands, events, errors, `Result`, references, unit, and arbitrary custom
output types are unsupported.

## Semantics

Queries do not mutate domain state, emit events, produce denials, or access persistence. The compiler enforces immutable root and input access. Persistence orchestration remains outside the domain query.

Multiple query groups may belong to one Aggregate. Query IDs are unique across the complete model and are projected in the top-level `queries` inventory. A query without business input has `input: null`; every query has an `output` descriptor.
