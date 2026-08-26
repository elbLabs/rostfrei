---
title: Action Outcome
kind: handbook
---

# Action Outcome

A public [Action](../reference/domain/action.md) ends with one outcome.

```text
Allowed
Denied
Technical Failure
```

## Allowed

Every Action declares an output contract.

An allowed Action returns the output declared by its Rust signature. An
infallible Action returns that output directly, including implicit `()` when no
return type is written. A fallible Action uses direct canonical
`Result<Output, Error>` syntax. Value Object constructors and transformations
return the Value Object itself on success.

An allowed [Aggregate](../reference/domain/aggregate.md) Action also returns
zero or more [Domain Events](../reference/domain/domain-event.md) owned by that
exact Aggregate. Entity and Value Object Actions do not return Domain Events.

An allowed [Domain Service](../reference/domain/domain-service.md) Action also
returns the Domain Events produced by coordinated Aggregate Actions when the
event-owning Aggregate has exactly the service's Context. Event ownership rules
apply through arbitrary `Option` and `Vec` nesting.

All owned state changes and Domain Events occur together.

## Denied

A denied Action returns a [Domain Error](../reference/domain/domain-error.md)
owned by that Action's owner. The compiler validates this for recognized
canonical `Result` signatures.

It returns no allowed output or Domain Events.

No state change occurs.

## Technical Failure

A technical failure is not a Domain Error.

It returns no allowed output or Domain Events.

No state change occurs.

## Internal Actions

Internal Entity and Value Object Actions return an internal allowed result or
internal denial to their parent. They do not return a public Action Outcome.
