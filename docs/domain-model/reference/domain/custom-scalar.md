---
title: Custom scalar
kind: reference
---

# Custom scalar

## Definition

A **Custom scalar** gives a custom Rust type scalar meaning in the domain model.
A provider associates that Rust type with a stable ID, a label, and one
canonical scalar representation.

Use a Custom scalar when the Rust type should remain visible as a distinct
semantic value instead of being modeled as its canonical representation alone.
For example, a `uuid::Uuid` can remain a UUID in the model while declaring that
its canonical representation is a string.

## Provider Declaration

Declare a provider type and implement the actual Rust API
`SemanticScalar` for it. `SemanticScalarDescriptor` supplies the scalar metadata:

```rust
use domain::{
    ScalarType, SemanticScalar, SemanticScalarDescriptor,
};
use uuid::Uuid;

struct UuidScalar;

impl SemanticScalar for UuidScalar {
    type Value = Uuid;

    const DESCRIPTOR: SemanticScalarDescriptor = SemanticScalarDescriptor {
        id: "uuid",
        label: "UUID",
        representation: ScalarType::String,
    };
}
```

The provider is separate from `Uuid`: this allows an application or shared
library to describe an external Rust type without implementing a foreign trait
for a foreign type. The provider is named in domain attributes and does not
replace the field's Rust type.

`id` is the stable semantic identifier, `label` is its display name, and
`representation` is one of the built-in `ScalarType` values. The representation
is the canonical data shape used to describe the Custom scalar; it does not
collapse the UUID into an ordinary `String` in the domain model.

## Derived Fields

On an Entity, Value Object, Domain Command, Domain Event, or Domain Error field,
select the provider with `#[domain(scalar = Provider)]`:

```rust
#[derive(ValueObject)]
#[domain(
    id = "delivery-references",
    label = "Delivery references",
    owner = Delivery,
)]
struct DeliveryReferences {
    #[domain(scalar = UuidScalar)]
    delivery_id: Uuid,

    #[domain(scalar = UuidScalar)]
    related_ids: Option<Vec<Option<Uuid>>>,
}
```

`Option` and `Vec` may nest to any supported depth. The attribute applies to the
innermost scalar value, while the descriptor preserves the wrappers from
outermost to innermost.

A Custom scalar field is explicit. An unannotated custom Rust type is not
inferred from a `SemanticScalar` implementation.

## Domain Identity

A Domain Identity puts the provider on its container attribute because the
tuple field itself does not accept domain metadata:

```rust
#[derive(DomainIdentity)]
#[domain(owner = Bicycle, scalar = UuidScalar)]
struct BicycleId(Uuid);
```

The resulting Domain Identity remains owned by exactly one Entity and projects
its Custom scalar descriptor. The Entity identity field continues to use
`#[domain(identity)]`; it does not repeat the provider.

## Compile-Time Type Equality

The derive-generated Rust bounds require the provider's associated
`SemanticScalar::Value` type to equal the annotated field's innermost type. For
the UUID provider above, these are valid:

```rust
#[domain(scalar = UuidScalar)]
value: Uuid,

#[domain(scalar = UuidScalar)]
values: Option<Vec<Uuid>>,
```

Using `UuidScalar` on a `String` field, or declaring a UUID Domain Identity over
a different tuple-field type, fails at compile time. This checks provider/type
equality; it does not inspect values at runtime.

## Canonical Representation and Runtime Limits

A Custom scalar descriptor is model metadata, not a runtime codec or validator:

- `representation: ScalarType::String` describes a UUID's canonical
  representation but does not convert a `Uuid` to or from `String`.
- `SemanticScalar` does not guarantee a Serde implementation or a particular
  serialized format. Transport and persistence adapters remain responsible for
  encoding and decoding.
- The provider does not validate field values. Constructors, Actions, and
  [Invariants](invariant.md) must still enforce non-nil UUIDs and any other
  business rules.
- A raw Custom scalar is not yet supported as an [Action](action.md) or
  [Query](query.md) input or output boundary. Custom scalars are supported in
  the annotated derived fields described above; use the boundary shapes allowed
  by the corresponding Action or Query contract.

In particular, declaring `UuidScalar` does not make `Uuid::nil()` invalid. A
constructor or invariant must reject it when non-nil identity is a business
requirement.

## Related Concepts

- A [Domain Identity](domain-identity.md) can use a Custom scalar as its value.
- [Entities](entity.md) and [Value Objects](value-object.md) can contain Custom
  scalar fields.
- [Domain Commands](domain-command.md), [Domain Events](domain-event.md), and
  [Domain Errors](domain-error.md) can carry Custom scalar fields.
