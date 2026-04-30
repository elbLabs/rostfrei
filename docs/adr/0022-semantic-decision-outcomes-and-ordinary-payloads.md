# ADR 0022: Semantic decision outcomes and ordinary payloads

## Status

Accepted.

## Context

[ADR 0016](0016-decision-policies-groups-and-outcomes.md) introduced typed
Decision inputs and detailed outcome payload descriptors. Those descriptors
classified parameters and enum fields as scalars or Value Objects and projected
their Rust shapes into the compiled domain model.

This duplicated information already enforced by Rust signatures and made
ordinary implementation-specific data require Rostfrei semantic tags. It also
created a Value Object reference inventory whose only purpose was validating
the inferred DTO metadata.

## Decision

Decision inputs are ordinary Rust function parameters. `domain_decisions`
preserves the authored signature but does not classify or project parameter
names or types.

A Decision must still return a type implementing `DecisionOutcomeType`.
`#[derive(DecisionOutcome)]` remains restricted to a non-generic, non-empty
enum. Every variant requires a unique stable `#[outcome(id = "...", label =
"...")]` tag. Unit, tuple, and struct variants may contain arbitrary Rust
payload fields; their fields and shapes are not domain metadata.

`DecisionDescriptor` contains the owner-scoped Decision ID, label, ordered
outcome descriptors, and implementation kind. Each
`DecisionOutcomeDescriptor` contains only its local ID and label. The compiled
model projects those semantic fields and no parameter or payload metadata.

The Decision-specific Value Object reference inventory and validation are
removed. Value Objects no longer implement Decision input or outcome-payload
adapter traits.

## Consequences

Decision and outcome code remains ordinary callable Rust, and the compiler
continues to prove that a tagged Decision returns a semantic outcome enum.
Applications may use domain types, DTOs, containers, or references where their
Rust design requires them without adding metadata solely for Rostfrei.

Compiled-model consumers must stop expecting Decision `parameters` and outcome
`shape` fields. Rostfrei deliberately does not provide a transport schema for
Decision calls or outcome payloads.

[ADR 0030](0030-singular-decisions-invariants-and-tests.md) subsequently
removes Decision groups, ownership, and projection. `DecisionOutcome` remains
the independent semantic enum contract described here.
