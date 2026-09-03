# ADR 0026: Semantic domain events

## Status

Accepted.

## Context

Domain-event declarations previously generated a separate definition object,
and application code used `DomainEventDefinitionType::DEFINITION` for local
metadata such as an event name. Aggregate membership was already expressed by
the authored `AggregateEvents` enum, making a second definition API needless.

## Decision

`DomainEvent` is the semantic event contract. A normal declaration contains a
local ID and label; schema version `1` is implicit:

```rust
#[derive(DomainEvent)]
#[domain(id = "bicycle-rented", label = "Bicycle rented")]
pub struct BicycleRented {
    bicycle_id: BicycleId,
}
```

An explicit `schema_version` is authored only when it differs from `1`.
`DomainEvent` exposes the local ID, label, fields, and schema version directly.
Application infrastructure that needs the local wire event name uses
`E::LOCAL_ID`.

Aggregate ownership is not declared on the event. `AggregateEvents` supplies
membership and combines the semantic event metadata with the aggregate ID for
owned descriptors, conversion, JSON encoding and decoding, application, and
replay. The event set remains the sole authority established by ADR 0023.

The typed filesystem continues to require one `DomainEvent` declaration in
`event.rs`. The declaration is owner-independent and omits a default schema
version.

## Consequences

This is a breaking API change for consumers of `DomainEventDefinition` and
`DomainEventDefinitionType`. They use the semantic `DomainEvent` constants
instead. Ordinary event source remains compact while explicit non-default
versions remain visible and testable.

The runtime wire contract is unchanged: event execution still applies
immediately, persisted records retain the declared schema version and canonical
JSON bytes, and replay decodes through the aggregate event set.

[ADR 0031](0031-entity-identity-accessor-and-opaque-fields.md) removes identity
and Value Object field-role tags from Events. Custom payload fields become
opaque descriptor values while their persisted JSON contract is unchanged.
