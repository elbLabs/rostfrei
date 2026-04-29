# ADR 0024: Handler-linked commands

## Status

Accepted.

## Context

`Command` declarations previously repeated executable relationships through
`owner`, `rejection`, `json`, and `runtime` attributes. A separate
`CommandDefinition` implementation or generated runtime module repeated the
aggregate relationship again. These paths could drift from the actual
`CommandHandler<C> for A` implementation.

The compiled domain model also carried an explicit command inventory even
though runtime registration already needs the aggregate and command types.

## Decision

A domain command declares its local identity, label, fields, and only a
non-default schema version:

```rust
#[derive(Command)]
#[domain(id = "rent-bicycle", label = "Rent bicycle")]
pub struct RentBicycle {
    #[domain(identity)]
    bicycle_id: BicycleId,
}
```

Schema version `1` is implicit. The derive supplies the conventional JSON
payload contract. It does not declare an owner, rejection, or separate runtime
mode.

The executable relationship has one authored path:

```rust
impl CommandHandler<RentBicycle> for RentalFleetAggregate {
    type Rejection = BicycleUnavailable;
    // ...
}
```

`CommandDefinition<A>` is a blanket bridge for a command `C` when `A`
implements `CommandHandler<C>`. Runtime registration and typed use name the
pair explicitly:

```rust
registry.register_command::<RentalFleetAggregate, RentBicycle>()?;
processor.register::<RentalFleetAggregate, RentBicycle>(rejection_mapper)?;
bus.dispatch::<RentalFleetAggregate, RentBicycle>(request).await?;
tracer.register_json::<RentalFleetAggregate, RentBicycle>()?;
```

The paired registry descriptor combines the aggregate runtime type with the
command's local ID, label, schema version, and field metadata. The tracer
catalog consumes that descriptor directly, so catalog commands retain their
aggregate type and modeled fields without a compiled-model command inventory.

`domain_model!` has no `commands` list and its JSON has no commands collection.
`DomainModule`, `ModuleDescriptor`, and `domain_module!` are removed; direct
paired registry calls are the complete registration mechanism.

## Consequences

This is a breaking source and API change. Applications remove command
`owner`, `rejection`, `json`, and `runtime` attributes, omit
`schema_version = 1`, and remove command inventories and runtime modules.
Command processors, buses, integration-event command contexts, and tracer
bindings require explicit aggregate-command pairing.

The compiler checks the aggregate, command, and rejection relationship at the
handler implementation. Registry and catalog identities remain fully scoped,
while duplicate handwritten relationship declarations disappear.
