# ADR 0029: Explicit domain service definitions

## Status

Accepted.

## Context

`DomainService` previously combined semantic identity, bounded-context
ownership, and manually attached action lists in one derive attribute. Context
is an executable type relationship, while action attachment is optional model
composition. Keeping both inside the derive repeated concerns and made omitted
versus intended capability projection unclear.

## Decision

`#[derive(DomainService)]` declares only the service's local ID and label:

```rust
#[derive(DomainService)]
#[domain(id = "fleet-planning", label = "Fleet planning")]
pub struct FleetPlanning;
```

An ordinary definition implementation supplies its bounded context:

```rust
impl DomainServiceDefinition for FleetPlanning {
    type Context = BikeRental;
}
```

The service derive has no action attachment. Registering a service contributes
its identity to the compiled model without implicitly projecting behavior.
Service actions are ordinary singular traits nested beneath the service as
defined by ADR 0028; actions are not a compiled-model inventory.

The typed filesystem recognizes a domain-service directory as a direct child
of a bounded context, anchored by `service.rs`. That file contains exactly one
`DomainService` declaration and one matching `DomainServiceDefinition`
implementation. A demonstrated service may contain singular action
directories. No broader service-owned hierarchy is introduced here.

## Consequences

This is a breaking source change. Applications remove `context` and `actions`
from service attributes and add `DomainServiceDefinition`.

Service identity and context remain compiler checked without making action
projection implicit. Tooling can enforce the minimal filesystem relationship
deterministically.

[ADR 0030](0030-trait-preserving-singular-domain-actions.md) removes action
extensions and action owner kinds altogether. The service definition and
filesystem location remain, while a nested service action is a singular
owner-independent trait whose conceptual owner is inferred from nesting.
