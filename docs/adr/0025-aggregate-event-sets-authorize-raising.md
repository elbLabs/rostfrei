# ADR 0025: Aggregate event sets authorize raising

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

### Event-raising scope

In configured domain roots, the structure checker reserves `raise` method calls
and qualified `::raise` expression references for the method bodies of validated
domain action implementations. The implementation must match the action declared
in the sibling `action.rs` and its enclosing aggregate, entity, value object, or
service owner. An arbitrary `execute.rs` file does not authorize event raising.
Handlers invoke actions directly on loaded handles, such as `account.credit(amount)`.

`cargo rostfrei-dev check --workspace` reports `RF011` for event raising in handlers,
queries, policies, apply/initialize implementations, free helper functions, nested
items, or other unapproved scopes. Closures in action methods retain their action
scope. Qualified references are checked even when assigned to function-pointer
aliases; renaming the receiver type does not hide a `.raise` or `::raise` reference.
Method/path references visible in macro input, attribute arguments, and macro
definitions are checked at their source location. Attributes on action methods
retain the action scope; attributes on helpers and entire implementations do not
grant that scope. Method calls include optional turbofish arguments; field access
such as `salary.raise` and ranges such as `0..raise` are not method calls.
String literals are not interpreted as code.

This is a source-level architectural rule: `raise` remains public for action
implementations in application crates. The checker reserves these spellings
within domain code without resolving receiver types or expanding macros. Code
emitted entirely by external or procedural macros, and calls through external
helper APIs that do not spell `raise` in the checked source, require separate
review. The mirrored `domain/tests` tree and code outside configured domain roots
are outside this rule, allowing low-level framework tests and fixtures.

## Consequences

This is a breaking source and model change. Applications remove every
`raises = [...]` action attribute. Tests that checked declared action outputs
move to the executable boundary: registered events execute and replay, while
unregistered events fail compilation at `AggregateInstance::raise`.

Event membership cannot drift between an action attachment and the aggregate
codec because there is no second list. Tooling loses speculative action/event
relationships and retains the stronger, compiler-checked aggregate event set.

[ADR 0030](0030-trait-preserving-singular-domain-actions.md) subsequently
removes plural action groups and owner metadata while preserving this direct
raising behavior on ordinary trait implementations.

The event declaration side of this relationship is simplified by
[ADR 0028](0028-semantic-domain-events.md): semantic local metadata lives on
`DomainEvent`, while this ADR's event set continues to supply aggregate
membership and runtime behavior.
