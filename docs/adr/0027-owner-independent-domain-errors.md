# ADR 0027: Owner-independent domain errors

## Status

Accepted.

## Context

`DomainError` declarations previously repeated their aggregate, entity,
domain-service, or value-object relationship through an `owner` attribute.
They also required a `json` flag to opt into the only supported rejection
payload path. These declarations could drift from the action return type or
the `CommandHandler` implementation that actually uses the error.

## Decision

A domain error declares its global identity, label, stable public code, message,
and Rust payload fields:

```rust
#[derive(DomainError)]
#[domain(
    id = "bicycle-unavailable",
    label = "Bicycle unavailable",
    code = "BICYCLE_UNAVAILABLE",
    message = "The requested bicycle cannot currently be rented."
)]
pub struct BicycleUnavailable {
    bicycle_id: BicycleId,
}
```

Domain-error IDs are owner-independent. The derive always supplies the
conventional JSON rejection payload; there is no second serialization path to
select.

Usage establishes relationships. An action's authored return type says which
error it can return. For commands, the handler is the authoritative link:

```rust
impl CommandHandler<RentBicycle> for RentalFleetAggregate {
    type Rejection = BicycleUnavailable;
    // ...
}
```

The stable code and message remain explicit because they are observable in
runtime command responses and form part of the application's public rejection
contract.

## Consequences

This is a breaking source change. Applications remove `owner` and `json` from
domain-error attributes. They no longer generate or consume owner-binding
metadata for errors. JSON encoding and runtime rejection behavior remain
available by default, while the compiler checks each executable relationship
where the error type is actually used.

[ADR 0033](0033-entity-identity-accessor-and-opaque-fields.md) removes identity
and Value Object field tags from Domain Errors. Custom rejection fields are
opaque metadata without changing their JSON payload behavior.
