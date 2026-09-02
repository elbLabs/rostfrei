# ADR 0023: Aggregate event sets authorize raising

## Status

Accepted.

## Context

Executable aggregate action declarations previously repeated a `raises = [...]`
list. The list projected possible action-to-event relationships but could not
prove that an implementation raised those events, nor that it did not raise
others. The implementation and the aggregate's closed event representation
were already the executable sources of truth.

## Decision

Action metadata contains no `raises` declaration:

```rust
#[domain_action(id = "rent-bicycle", label = "Rent bicycle")]
trait RentalFleetActionContract {
    fn rent_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleUnavailable>;
}
```

An implementation raises concrete events through `AggregateInstance::raise`.
That method requires conversion into the aggregate's authored
`AggregateDefinition::Event` type. `#[derive(AggregateEvents)]` generates those
conversions only for enum members, so raising an unregistered event fails to
compile through the missing `From<Event>`/`Into<AggregateEvents>` bound.

The aggregate event set is the sole authority for event membership, runtime
application, persistence codecs, replay, and compiled event projection. Action
descriptors no longer contain possible-event lists, and the compiled model does
not claim action-to-event edges that Rust cannot verify.

## Consequences

This is a breaking source and model change. Applications remove every
`raises = [...]` action attribute. Tests that checked declared action outputs
move to the executable boundary: registered events execute and replay, while
unregistered events fail compilation at `AggregateInstance::raise`.

Event membership cannot drift between an action attachment and the aggregate
codec because there is no second list. Tooling loses speculative action/event
relationships and retains the stronger, compiler-checked aggregate event set.

[ADR 0028](0028-trait-preserving-singular-domain-actions.md) subsequently
removes plural action groups and owner metadata while preserving this direct
raising behavior on ordinary trait implementations.

The event declaration side of this relationship is simplified by
[ADR 0026](0026-semantic-domain-events.md): semantic local metadata lives on
`DomainEvent`, while this ADR's event set continues to supply aggregate
membership and runtime behavior.
