---
title: Domain Identity
kind: reference
---

# Domain Identity

## Definition

A **Domain Identity** is the stable typed identity of exactly one [Entity](entity.md), including an aggregate root entity.

## Model Shape

```rust
#[derive(DomainIdentity)]
#[domain(owner = Bicycle)]
pub struct BicycleId(String);
```

A Domain Identity is a non-generic tuple struct with exactly one field. The field
may be a canonical scalar (`bool`, `String`, `char`, a fixed-width integer,
`isize`, `usize`, `f32`, or `f64`) or an explicitly provided
[Custom scalar](custom-scalar.md).

`owner` is required and identifies the exact Entity whose identity field uses
the type. A Custom scalar identity also declares
`#[domain(owner = Entity, scalar = Provider)]`; the provider's
`SemanticScalar::Value` must equal the tuple-field type. Domain attributes on
the field, wrappers, references, unannotated custom types, enums, and unions are
unsupported. Canonical identities do not support aliases because their scalar is
recognized syntactically. A Custom scalar identity may use a direct type alias
when the provider's associated `Value` resolves to that same Rust type.

## Projection

`DomainIdentityDescriptor` records the owner Entity ID and canonical
`ScalarType`. For a Custom scalar identity, `DomainIdentityType::SEMANTIC_SCALAR`
preserves the provider descriptor and the compiled model projects that semantic
metadata instead of only the canonical representation. Entity identity
descriptors and every identity-valued field project the same typed identity ID.
The compiled model inventories identities in `domainIdentities`.

## Boundaries

A Domain Identity carries no state access or behavior. It identifies only its declared Entity and cannot be reused as another Entity's identity.
