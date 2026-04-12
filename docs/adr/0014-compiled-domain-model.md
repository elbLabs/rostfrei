# ADR 0014: Compiled domain model

## Status

Accepted.

## Decision

Rostfrei absorbs the former standalone domain compiler as its canonical,
optional domain-model platform layer. The imported implementation becomes
`rostfrei-domain`, `rostfrei-domain-macros`, and Rostfrei Studio. Its
domain-language reference, handbook, examples, compile-time tests, and model
browser are maintained in the Rostfrei repository and namespace.

The event-sourcing kernel remains independent. `rostfrei-core` does not depend
on domain descriptors, procedural macros, model projection, Studio, or a UI
protocol. Applications can continue to use the kernel without the compiled
model.

`rostfrei-domain-runtime` is the integration boundary. A descriptive
`AggregateType` explicitly maps to an executable `rostfrei-core::Aggregate`.
Model-owned commands infer that aggregate owner, retain their rich
`DomainCommandDescriptor`, and add the runtime-only command name and schema
version required by `DomainRegistry`.

Domain actions are not automatically invoked as event-sourced command handlers.
The model permits actions that mutate an aggregate root directly, while the
Rostfrei kernel changes state by recording and applying events. Runtime
handlers therefore remain explicit until one coherent execution contract is
defined.

## Consequences

Rostfrei has one source of truth for domain identity, structure, ownership,
behavior metadata, testing metadata, and Studio presentation. It does not need
a permanent compatibility adapter to a separately evolving compiler project.

The imported model is broader than the current runtime. Some contracts remain
descriptive, model assembly still produces unversioned JSON and can panic on
invalid inventories, and Studio is currently a read-only browser. Those are now
Rostfrei platform concerns to evolve without expanding the kernel's
responsibilities.
