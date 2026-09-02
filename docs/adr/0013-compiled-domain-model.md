# ADR 0013: Compiled domain model

## Status

Accepted.

## Decision

rostfrei absorbs the former standalone domain compiler as its canonical,
optional domain-model platform layer. The imported implementation becomes
`rostfrei-domain` and `rostfrei-domain-macros`. Its domain-language reference,
handbook, examples, and compile-time tests are maintained in the rostfrei
repository and namespace.

The event-sourcing kernel remains independent. `rostfrei-core` does not depend
on domain descriptors, procedural macros, model projection, or tooling
protocols. Its manual `Aggregate` and `EventCodec` contracts remain available to
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
Applications declare executable aggregate Actions that explicitly raise
concrete events and never declare or use the generated representation. Raising
an event applies it immediately and records it as uncommitted.

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

Executable aggregate Actions use `&mut self`, domain-specific input, and
`()`/`Result<(), DomainError>` outcomes.
`domain_actions(aggregate(instance = ...))` generates the trait implemented for
`AggregateInstance`; Action implementations explicitly call `self.raise(...)`
zero or more times. `raises = [...]` declares possible event types for the
compiled model, while the Aggregate's `events = [...]` remains the executable
event inventory. Commands remain an application boundary and map their payloads
into one or more Action inputs rather than being passed to Actions directly.

`Command` derives the runtime command definition from its owner, local ID,
and schema version. Registering a command runtime binding inserts that descriptor
into the registry when it is not already present. `domain_module!` remains an
optional grouping mechanism, not a prerequisite for command registration and
not a declaration of every modeled domain capability. Decision metadata is
attached to explicit inherent aggregate or entity impl blocks.

The Decision-specific attachment, signature, outcome, testing, and model shape
contracts were subsequently refined by
[ADR 0015](0015-decision-policies-groups-and-outcomes.md). Its explicit groups
and `DecisionOutcome` enum contract supersede earlier single-block and
`Result`-shaped Decision details without superseding the rest of this ADR.

The Aggregate-specific declaration and event-membership contracts were
subsequently replaced by
[ADR 0018](0018-aggregate-definition-and-event-set.md). Its explicit
`AggregateDefinition` implementation and authored `AggregateEvents` enum
supersede this ADR's `events = [...]` attachment and generated hidden event
representation. ADR 0018 also supersedes aggregate-level action, decision, and
invariant attachment and automatic invariant fanout; those relationships are
intentionally absent until Rostfrei can derive or validate them without manual
lists.

The Entity-specific declaration, lifecycle, and invariant contracts were
subsequently replaced by
[ADR 0019](0019-explicit-entity-definition-and-owner-independent-tags.md).
Its explicit `EntityDefinition`, owner-independent lifecycle and invariant
metadata, and absence of implicit entity capability projection supersede this
ADR's entity attachments and owned lifecycle/invariant model relationships.

Domain identity ownership, representation, and model discovery were
subsequently simplified by [ADR 0020](0020-slim-domain-identities.md). Identity
newtypes are marker-derived and discovered through `EntityDefinition`; they no
longer have a separate model inventory or inferred scalar metadata.

Value-object ownership, shape projection, and operation DTO contracts were
subsequently simplified by
[ADR 0021](0021-slim-value-objects-and-ordinary-dtos.md). Value objects now
carry semantic ID and label metadata only; ordinary action/query DTOs are not
promoted into the compiled domain model.

## Consequences

rostfrei has one source of truth for domain identity, structure, event
membership, ownership, default persistence behavior, behavior metadata, and
testing metadata. It does not need a permanent compatibility adapter to a
separately evolving compiler project or a second public event macro system.

The imported model is broader than the current runtime. Some contracts remain
descriptive, and model assembly can still panic on invalid non-event
inventories.
Those are now rostfrei platform concerns to evolve without expanding the
kernel's responsibilities.
