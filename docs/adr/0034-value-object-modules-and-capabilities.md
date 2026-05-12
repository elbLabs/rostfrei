# ADR 0034: Value Object modules and capabilities

## Status

Accepted.

The reusable Decision capability and `decision.rs` terminology discussed
historically below is superseded by
[ADR 0036](0036-domain-policy-vocabulary.md): these are Domain Policy and
`policy.rs`.

## Context

Value Objects may begin as small semantic types and later acquire meaningful
behavior. A leaf such as `status.rs` is concise initially, but moving it into a
directory later changes its Rust module path and creates avoidable review churn.
The typed filesystem also needs one deterministic owner anchor when actions,
invariants, or decisions belong to the value itself.

## Decision

Every Value Object is a module. Its directory contains a composition-only
`mod.rs` and exactly one declaration in `value.rs`:

```text
status/
├── mod.rs
└── value.rs
```

Value Object directories may be direct children of a bounded context,
aggregate, or entity. They may contain action, invariant, and decision
capability directories:

```text
registration_number/
├── mod.rs
├── value.rs
├── normalize/
│   ├── mod.rs
│   ├── action.rs
│   └── execute.rs
├── validity/
│   ├── mod.rs
│   ├── contract.rs
│   └── evaluate.rs
└── choose_format/
    ├── mod.rs
    ├── decision.rs
    ├── outcome.rs
    └── evaluate.rs
```

`value.rs` contains one `ValueObject` declaration, imports, and inherent
implementations for that type. Each child capability remains an ordinary
singular trait. Its `execute.rs` or `evaluate.rs` must implement that trait
directly for the Value Object declared by the parent `value.rs`.

Queries, lifecycles, entities, services, aggregates, and nested Value Objects
are not valid Value Object children. Operation DTOs remain ordinary Rust types
and do not become Value Objects merely because they are inputs or outputs.

Tests remain in the sibling domain test tree. A test file may mirror `value.rs`
directly or map to a child capability directory, preserving the existing
selective test-mirror rule.

## Consequences

Simple and behaviorful Value Objects now share one stable module shape. Adding
behavior no longer moves or renames the semantic declaration.

The `ValueObject` derive remains thin: it still generates only the global ID
and label descriptor. Ownership is inferred by the structure checker from the
directory tree and verified against direct Rust trait implementations.

Existing leaf Value Objects must move from `<name>.rs` to
`<name>/{mod.rs,value.rs}`. Their public re-exports and serialized
representations can remain unchanged.
