---
title: Domain Service
kind: reference
---

# Domain Service

## Definition

A **Domain Service** is a stateless domain object that coordinates behavior
across aggregates in one [Bounded Context](bounded-context.md).

## Responsibility

A domain service owns:

- its public service actions
- its attached coordination decisions

A service invokes public [Aggregate](aggregate.md) actions. It does not invoke
an [Entity](entity.md) or [Value Object](value-object.md) directly. Because it
owns no state, it cannot own or attach invariant contracts; there is no
`#[domain_invariants(domain_service)]` contract kind.

When an aggregate action denies, the service translates that denial into a
service-owned [Domain Error](domain-error.md).

A service Action may return Domain Events from any Aggregate whose Context is
exactly the service Context, including events nested in `Option` and `Vec`. It
cannot return an event from an Aggregate in another Context.

A service may coordinate multiple Aggregate interactions, but one service
Action changes the state of at most one Aggregate. Follow-up Aggregate changes
occur through Domain Events and separate public Actions.

A Domain Service coordinates at least two distinct Aggregates. It completes
read-only Aggregate interactions before invoking its one state-changing
Aggregate Action.

## Decisions

Domain Service Decision contracts use `#[domain_decisions(domain_service)]`.
The service implements each contract attached through
`decisions = [TraitPath, ...]`. A Domain Service Action may also call any other
visible Decision in the same Bounded Context. The compiler does not enforce
Action call permissions. See [Decision](decision.md).

## Actions

Domain Service actions are declared in an unrestricted public contract trait:

```rust
#[domain_actions(domain_service)]
pub trait TransferActions {
    #[action(id = "transfer", label = "Transfer funds")]
    fn transfer(input: TransferFunds)
        -> Result<Option<Vec<FundsTransferred>>, TransferDenied>;
}
```

Every method is an associated function with no receiver and zero or one business
parameter named `input`. A command input must be owned by that exact Domain
Service, and a returned error must be owned by that exact service. Successful
outputs may include Domain Events owned by Aggregates in the service's Context,
including through supported `Option` and `Vec` wrappers; events from another
Context are rejected.

Attach contracts through `actions = [TraitPath, ...]` on the Domain Service. The
service must implement each attached trait; implementing a trait without
attaching it does not project its actions. Import the trait for an
owner-associated call, or use fully qualified syntax without an import:

```rust
use contracts::TransferActions as _;
TransferService::transfer(input);

<TransferService as contracts::TransferActions>::transfer(input);
```

The Domain Service derive exposes attached descriptors through
`DomainServiceType::ACTION_CONTRACTS`. Registering the service in `domain_model!`
automatically projects them after Aggregate, Entity, and Value Object actions.
Within Domain Services, model `services` inventory order comes first, followed
by `actions` attachment order and trait method source order. See
[Action](action.md) for trusted descriptor-extension rules.

## Boundaries

A domain service does not:

- own aggregate state or invariant contracts
- call a Decision from another Bounded Context
- change aggregate state directly
- coordinate aggregates outside its bounded context

## Related Concepts

- A [Bounded Context](bounded-context.md) contains domain services.
- An [Aggregate](aggregate.md) owns state and exposes public behavior.
- An [Action](action.md) may be owned by a domain service.
- A [Decision](decision.md) may be owned by a domain service.
