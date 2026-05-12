# ADR 0032: Singular decisions, invariants, and normalized tests

## Status

Accepted.

The reusable-rule vocabulary in this historical decision is superseded by
[ADR 0036](0036-domain-policy-vocabulary.md): `domain_decision`,
`DecisionOutcome`, and `decision.rs` become `domain_policy`, `PolicyOutcome`,
and `policy.rs`.

## Context

Decision groups and plural invariant contracts retained generated markers,
references, owner kinds, and attachment concepts after their inputs and outputs
became ordinary Rust. Their behavior was already expressed by traits and
implementations, while domain-test subject syntax still depended on generated
owner-associated reference constants.

## Decision

Each Decision is one preserved ordinary trait:

```rust
#[domain_decision(id = "assess-rental", label = "Assess rental")]
trait RentalAssessmentDecision {
    fn assess_rental(&self) -> RentalAssessment;
}

impl RentalAssessmentDecision for RentalFleetAggregate {
    fn assess_rental(&self) -> RentalAssessment { /* ... */ }
}
```

Each Invariant follows the same form:

```rust
#[domain_invariant(id = "fleet-consistency", label = "Fleet consistency")]
trait FleetConsistency {
    fn validate(candidate: &RentalFleet) -> Option<InvariantViolation>;
}

impl FleetConsistency for RentalFleetAggregate {
    fn validate(candidate: &RentalFleet) -> Option<InvariantViolation> { /* ... */ }
}
```

Both attributes add `LOCAL_ID`, `LABEL`, and `DESCRIPTOR` associated constants.
They preserve the authored Rust signature and do not declare an owner or group.
Implementations are direct on the enclosing aggregate or entity. Decision
outcome enums continue to derive `DecisionOutcome`; their ordered semantic
alternatives remain available independently of decision projection.

The compiled model does not project Decisions or Invariants. There are no
groups, attachments, owner marker traits, references, or inventories.

Domain-test macros use one normalized descriptor expression:

```rust
#[domain_decision_test(<RentalFleetAggregate as RentalAssessmentDecision>::DESCRIPTOR)]
#[domain_invariant_test(<RentalFleetAggregate as FleetConsistency>::DESCRIPTOR)]
```

The typed filesystem recognizes `domain_decision` in `decision.rs` and
`domain_invariant` in `contract.rs`. Their `evaluate.rs` must contain exactly
one direct, unqualified, unaliased implementation for the enclosing aggregate
or entity. Missing or duplicate implementations, wrong owners, aliases,
qualified paths, and glob imports are rejected. Private helpers remain valid.

## Consequences

This is a breaking source and model change. Applications replace plural macros
and grouped/inherent implementations with singular traits, direct
implementations, and ordinary trait calls. Tests reference the selected
implementor/trait descriptor explicitly.

Decision and invariant behavior is now normal Rust with a deterministic
filesystem relationship. Metadata remains semantic and owner-independent,
while `DecisionOutcome` continues to model closed business vocabularies.
