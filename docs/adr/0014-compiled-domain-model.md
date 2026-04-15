# ADR 0014: Compiled domain model

## Status

Accepted.

## Decision

rostfrei absorbs the former standalone domain compiler as its canonical,
optional domain-model platform layer. The imported implementation becomes
`rostfrei-domain`, `rostfrei-domain-macros`, and rostfrei Studio. Its
domain-language reference, handbook, examples, compile-time tests, and model
browser are maintained in the rostfrei repository and namespace.

The event-sourcing kernel remains independent. `rostfrei-core` does not depend
on domain descriptors, procedural macros, model projection, Studio, or a UI
protocol. Its manual `Aggregate` and `EventCodec` contracts remain available to
applications that intentionally use the kernel without the compiled model.

`rostfrei-domain-runtime` is the integration boundary, exposed to applications
through the `rostfrei` facade. The canonical compiled `Aggregate` declaration is
also the executable aggregate definition. Its root is the runtime state, and an
explicit `Initialize<Aggregate>` implementation constructs that root from the
stream identity before replay. There is no separately declared runtime
aggregate. Its stream aggregate type is the context-scoped domain identity
`<bounded-context-id>/<aggregate-id>`, so equal local aggregate IDs in different
contexts cannot share streams or handler registrations.

`DomainEvent` describes owner-independent event metadata: local ID, label,
schema version, fields, and the default JSON payload contract. An aggregate's
`events = [...]` attachment assigns ownership and is the single source of event
membership. The aggregate derive generates the owned descriptors, a doc-hidden
aggregate event representation, concrete event conversions, `Apply<Event>`
dispatch to the declared root, JSON encoding and fail-closed replay decoding.
Applications declare executable aggregate Actions that return concrete events
and never declare or use the generated representation. The generated
`AggregateInstance` adapter raises successful Action results, applies them
immediately, and records them as uncommitted events.

`domain_model!` projects each aggregate's attached events automatically in
aggregate and attachment order. It has no flat event inventory, so an event
cannot drift between runtime registration and model projection. Conflicting
ownership is rejected by Rust coherence, duplicate local event IDs are rejected
during compilation, and duplicate projected identities are rejected by model
assembly.

`Executor::new(store)` and normal committed-event-handler registration select
the generated JSON codec. `Executor::with_codec` and explicit handler codec
registration preserve custom DTO, legacy schema, upcasting, Protobuf, and other
format needs. Unknown event types, unsupported schema versions, and malformed
payloads fail replay closed.

Normal applications depend on the `rostfrei` facade and use `rostfrei::Aggregate`,
`rostfrei::DomainEvent`, `rostfrei::Apply`, `rostfrei::Initialize`, and
`rostfrei::Executor` with `#[rostfrei(...)]`. Implementation crates and generated
event representations are not part of normal application syntax.

Executable aggregate Actions use an immutable root, domain-specific input, and a
direct aggregate-owned event result. `domain_actions(aggregate(instance = ...))`
generates extension methods on `AggregateInstance` from the same contract and
implementation used for model metadata. The generated adapter raises only a
successful event result; rejected Actions leave state and uncommitted events
unchanged. Commands remain an application boundary and map their payloads into
one or more Action inputs rather than being passed to Actions directly.

`DomainCommand` derives the runtime command definition from its owner, local ID,
and schema version. Registering a command runtime binding inserts that descriptor
into the registry when it is not already present. `domain_module!` remains an
optional grouping mechanism, not a prerequisite for command registration and
not a declaration of every modeled domain capability.

## Consequences

rostfrei has one source of truth for domain identity, structure, event
membership, ownership, default persistence behavior, behavior metadata, testing
metadata, and Studio presentation. It does not need a permanent compatibility
adapter to a separately evolving compiler project or a second public event
macro system.

The imported model is broader than the current runtime. Some contracts remain
descriptive, model assembly can still panic on invalid non-event inventories,
and Studio is currently a read-only browser. Those are now rostfrei platform
concerns to evolve without expanding the kernel's responsibilities.
