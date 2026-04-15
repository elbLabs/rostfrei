---
title: Domain Error
kind: reference
---

# Domain Error

## Definition

A **Domain Error** is a modeled business denial.

It describes why a domain action or lifecycle rule was not allowed. An
invariant checker returns `InvariantViolation`, not a Domain Error; the Action
translates the complete violation collection into its own Domain Error. A
Decision similarly returns business-denial data in its output Value Object for
an Action to translate.

## Ownership

A domain error belongs to exactly one owner:

- [Domain Service](domain-service.md)
- [Aggregate](aggregate.md)
- [Entity](entity.md)
- [Value Object](value-object.md)

Only behavior owned by the same domain object may produce the error.

When an Action uses direct `Result<Output, Error>`, including the canonical
`core::result::Result` and `std::result::Result` paths, its `Error` must implement
`DomainErrorType` with the Action owner as its associated `Owner`.

Aggregate-owned and domain-service-owned errors are public boundary denials.
Entity-owned and value-object-owned errors are internal denials. Their owning
aggregate translates them into an aggregate-owned error.

## Behavior

A domain error defines a stable business contract for a denial. It declares:

- an error code
- a message

It may also include:

- fields that identify invalid input or state
- details that explain the denial

An owner may reuse the same domain error for multiple local denials. The rule
that produces the error supplies the specific fields or details.

`DomainError` supports non-generic structs. Its descriptor projects canonical
scalar, [Custom scalar](custom-scalar.md), identity, Value Object, and
aggregate-reference fields, including nested `Vec` and `Option` wrappers.
Contained Entity fields are invalid.

The opt-in `json` flag generates the control-plane rejection representation.
It always contains the descriptor's canonical `code` and `message`, followed by
the modeled fields. Those two names are therefore reserved for JSON-enabled
domain errors. Applications can use an explicit command wire codec when they
need another rejection representation.

An aggregate action translates entity-owned or value-object-owned errors into
an aggregate-owned error. A domain-service action translates aggregate-owned
errors into a service-owned error.

An owner-defined error may include a deterministic `violations` list when one
operation reports multiple invariant failures or other correctable denials. The
Action translates the complete `Vec<InvariantViolation>` into this public or
internal error shape. Exposed items use owner-defined paths and reasons, never
compiler-neutral or child error identities.

## Boundaries

A domain error does not represent:

- an infrastructure failure
- an unavailable dependency
- invalid transport data
- an implementation exception

Those are technical failures, not domain behavior.

## Model Shape

```yaml
id: TodoAggregate.Todo.TitleInvalid
code: TODO_TITLE_INVALID
message: Todo title is invalid.
```

## Related Concepts

- An [Action](action.md) may return a domain error.
- An [Invariant](invariant.md) returns violation data that an Action translates.
- A [Lifecycle](lifecycle.md) may deny an unavailable transition.
