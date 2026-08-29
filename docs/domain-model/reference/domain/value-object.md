---
title: Value Object
kind: reference
---

# Value Object

## Definition

A **Value Object** is a domain object defined by its value, not by independent
identity.

It belongs to a [Bounded Context](bounded-context.md), [Aggregate](aggregate.md),
or [Entity](entity.md).

A bounded-context-owned Value Object is a shared definition only within that
context. Shared does not mean cross-context. Aggregate-owned and Entity-owned
Value Objects remain local to their owner.

## Responsibility

A value object owns:

- its value shape
- its attached invariant contracts
- its internal actions

A Value Object Action may call any visible Decision in the same Bounded Context.
The compiler does not enforce Action call permissions. Attached invariant
contracts remain local to the Value Object.

A Value Object cannot own a Decision or Decision group. A supported Value Object
may be passed to a Decision by value or top-level immutable reference; both forms
produce the same Decision input metadata. A `DecisionOutcome` tuple or named
variant may also carry a Value Object as a payload field. The outcome enum is a
Decision return contract, not itself a Value Object, and its variants do not use
Value Object variant metadata.

## Invariants

Value Object invariant contracts use `#[domain_invariants(value_object)]`. The
Value Object implements each contract attached through
`invariants = [TraitPath, ...]`; all attached traits form its one complete
invariant set. Checkers receive `&<Self as InvariantOwnerType>::Candidate`,
which is `&Self`, and return `Option<InvariantViolation>`.

The Value Object Action explicitly calls the canonical owner validator and
translates the complete violations into its Value-Object-owned Domain Error
before returning the replacement value. Registering the Value Object in
`domain_model!` automatically projects attached invariants in attachment then
trait method order. See [Invariant](invariant.md).

## Actions

Value Object actions are internal contracts declared with
`#[domain_actions(value_object)]`. The trait must have inherited or restricted
visibility; unrestricted `pub` is rejected.

```rust
#[domain_actions(value_object)]
trait TodoTitleActions {
    #[action(id = "new", label = "Create title")]
    fn new(input: String) -> Result<Self, TodoTitleDenied>;

    #[action(id = "normalize", label = "Normalize title")]
    fn normalize(self) -> Self;
}

#[derive(ValueObject)]
#[domain(
    id = "todo-title",
    label = "Todo title",
    owner = TodoAggregate,
    actions = [TodoTitleActions],
)]
struct TodoTitle(String);
```

A constructor is an associated function with exactly one business `input`. A
transformation consumes `self` and accepts zero or one business `input`;
borrowed and typed receivers are unsupported. On success, either form must
return the exact owning Value Object type, directly or as the output of a direct
canonical `Result`. An equivalent type alias is accepted, but scalars, unit,
other Value Objects, Domain Events, and wrappers such as `Option<Self>` or
`Vec<Self>` are not. A fallible action's error must be owned by that exact Value
Object.

Attach contracts with `actions = [TraitPath, ...]`; the Value Object must
implement every attached trait. Implementing a trait does not attach it. With
the trait in scope, constructors use owner-associated syntax and transformations
use method syntax. Fully qualified trait syntax works without an import.

The Value Object derive exposes attached descriptors through
`ValueObjectType::ACTION_CONTRACTS`. Registering the Value Object in
`domain_model!` automatically projects them after Aggregate and Entity actions
and before Domain Service actions. Within Value Objects, model inventory order
comes first, followed by attachment order and trait method source order. An
omitted or empty `actions` list, an unattached trait, or a Value Object omitted
from the model does not project actions. See [Action](action.md) for trusted
descriptor-extension rules.

The owning Aggregate or Entity applies the returned value and emits any Domain
Event. Value Object actions do not emit or return Domain Events.

## Model Shape

```rust
#[derive(ValueObject)]
#[domain(id = "todo-title", label = "Todo title", owner = TodoAggregate)]
struct TodoTitle(String);
```

A closed set of domain values uses an all-unit enum:

```rust
#[derive(ValueObject)]
#[domain(id = "bicycle-condition", label = "Bicycle condition", owner = RentalFleetAggregate)]
enum BicycleCondition {
    Serviceable,
    MaintenanceRequired,
}
```

An enum whose variants are all unit variants is classified as a fieldless enum.
If any variant is a tuple or struct variant, the whole enum is classified as a
tagged enum, including any unit variants it also contains:

```rust
#[derive(ValueObject)]
#[domain(id = "contact-method", label = "Contact method", owner = Customer)]
enum ContactMethod {
    Unknown,
    Email(String),
    Sms {
        number: String,
        verified: bool,
    },
}
```

Unit, tuple, and struct variants preserve their distinct shapes. Rust also
distinguishes a unit variant from empty tuple and struct variants:

```rust
enum Marker {
    Unit,
    EmptyTuple(),
    EmptyStruct {},
}
```

`Unit` has unit shape and no fields. `EmptyTuple()` and `EmptyStruct {}` make the
enum tagged and have tuple and struct shape respectively, each with an empty
field list.

Variant fields use the same types, wrappers, annotations, validation, and source
ordering as normal Value Object fields. Named variant fields use normalized Rust
names. Tuple variant fields use `"0"`, `"1"`, and so on independently within
each variant. Field annotations belong on the fields, not on the variant:

```rust
enum DeliveryChoice {
    Pickup(#[domain(value_object)] PickupPoint),
    Ship {
        #[domain(value_object)]
        address: DeliveryAddress,
        instructions: Option<String>,
    },
}
```

Struct and variant field descriptors record fields in source order. Untagged
fields support `bool`, `String`, `char`, every fixed-width signed and unsigned
integer, `isize`, `usize`, `f32`, and `f64`. An explicitly annotated
`#[domain(scalar = Provider)]` field uses a
[Custom scalar](custom-scalar.md).

Nested values use `#[domain(value_object)]`. `#[domain(identity)]` marks a field
whose value implements `DomainIdentityType`; it does not give the Value Object
independent identity. Opaque references to another aggregate use
`#[domain(aggregate_ref = AggregateType)]`. Neither a Value Object struct nor a
tagged enum payload may contain an Entity. `Vec` and `Option` may nest to any
depth; their outermost-to-innermost structure is modeled without Rust type
strings.

References in Value Object struct fields and tagged variant payload fields must
also be registered in the same `domain_model!`: identity targets in
`identities`, nested Value Objects in `value_objects`, and aggregate-reference
targets in `aggregates`. `entities` is the corresponding inventory for Entity
fields on owner types that permit them; Value Objects do not. Reference
validation runs after all inventories are registered, so declaration order does
not matter and forward or cyclic references between registered Value Objects
are supported.

Only canonical `Vec`, `Option`, and `String` paths are recognized. Type aliases,
custom containers, maps, sets, references, arrays, slices, tuple types, generic
base types, and untagged custom types are unsupported as fields. A tuple enum
variant is supported because it declares separate variant fields; this does not
add support for a tuple type used as one field. For example, `Pair(String, bool)`
is a supported two-field tuple variant, while `Pair((String, bool))` has one
unsupported tuple-typed field.

Value Object enums must be non-generic and non-empty. Explicit discriminants and
variant-level `#[domain(...)]` metadata are unsupported. Standard derives and
unrelated Rust attributes remain available.

Fieldless and tagged enum descriptors preserve variant Rust names, casing, and
declaration order. Tagged enum model JSON emits `variants` plus `variantShapes`
and no top-level `fields` key. For the `ContactMethod` example, the
shape-specific JSON is:

```json
{
  "variants": ["Unknown", "Email", "Sms"],
  "variantShapes": [
    {
      "name": "Unknown",
      "kind": "unit"
    },
    {
      "name": "Email",
      "kind": "tuple",
      "fields": [
        {
          "name": "0",
          "value": {
            "kind": "scalar",
            "scalar": "string"
          }
        }
      ]
    },
    {
      "name": "Sms",
      "kind": "struct",
      "fields": [
        {
          "name": "number",
          "value": {
            "kind": "scalar",
            "scalar": "string"
          }
        },
        {
          "name": "verified",
          "value": {
            "kind": "scalar",
            "scalar": "bool"
          }
        }
      ]
    }
  ]
}
```

Every tagged variant has one `variantShapes` entry. Unit entries omit `fields`;
tuple and struct entries include `fields`, including `fields: []` for their empty
forms.

The fieldless enum output is unchanged: it emits only `variants`, with no
`variantShapes` or `fields` key. The `CategoryKind` shape-specific JSON remains:

```json
{
  "variants": ["Service", "Resource"]
}
```

Struct JSON continues to emit `fields` and no `variants` or `variantShapes` key.
A unit struct remains a struct with `fields: []`.

## Boundaries

A value object does not:

- have independent identity
- expose public actions
- have persisted state or a lifecycle
- emit events
- call a Decision from another Bounded Context
- invoke state-changing behavior outside its owned value space
- coordinate entities or aggregates

A Value Object may hold opaque IDs for Aggregates in the same
[Bounded Context](bounded-context.md). It does not access their state directly.
A Value Object may compose visible Value Objects without changing the nested
Value Object's declared ownership. Visibility and scope enforcement are
deferred.

## Related Concepts

- A [Bounded Context](bounded-context.md) may own Value Object definitions shared
  within that context.
- An [Aggregate](aggregate.md) or [Entity](entity.md) may own local Value Objects.
- A [Decision](decision.md) may take a Value Object as input or carry one in an
  outcome payload, but a Value Object cannot own Decisions.
- An [Action](action.md) may invoke value-object behavior internally.
