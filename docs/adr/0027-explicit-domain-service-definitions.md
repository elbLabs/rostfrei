# ADR 0027: Explicit domain service definitions

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

The service derive has no action attachment. When service behavior is intended
in a compiled model, composition registers both the service and the explicit
action extension:

```rust
domain_model! {
    services: [FleetPlanning],
    action_extensions: [FleetPlanningActions],
    // ...
}
```

Omitting either inventory is meaningful. An omitted service is absent from the
model; an omitted extension does not project implicit service actions.
Registration order remains authored model order.

The typed filesystem recognizes a domain-service directory as a direct child
of a bounded context, anchored by `service.rs`. That file contains exactly one
`DomainService` declaration and one matching `DomainServiceDefinition`
implementation. A demonstrated service may contain action directories whose
contracts declare the `domain_service` owner kind. No broader service-owned
hierarchy is introduced here.

## Consequences

This is a breaking source change. Applications remove `context` and `actions`
from service attributes, add `DomainServiceDefinition`, and explicitly list
intended service action extensions in `domain_model!`.

Service identity and context remain compiler checked without making action
projection implicit. Tooling can enforce the minimal filesystem relationship
deterministically and reject action-owner mismatches beneath a service.
