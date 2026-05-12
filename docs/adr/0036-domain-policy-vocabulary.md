# ADR 0036: Domain Policy vocabulary

## Status

Accepted.

## Context

Rostfrei used **Decision** for two different concepts. The ubiquitous language
defines a Decision as the deterministic result of handling one command: a
rejection or an ordered set of new domain events. The domain crate also used
`domain_decision` and `DecisionOutcome` for reusable, pure rules that actions
and queries may evaluate independently of a command.

The overlap made architecture discussions and APIs ambiguous. A rental
eligibility rule, for example, interprets bicycle facts but does not itself
accept a command, reject it, or record an event.

## Decision

**Decision** retains its command-scoped event-sourcing meaning from
[ADR 0001](0001-ubiquitous-language-and-scope.md). A reusable, pure
interpretation of domain facts is a **Domain Policy**.

A Domain Policy:

- has ordinary Rust inputs;
- returns a business outcome;
- does not mutate domain state or record events; and
- may be reused by actions, command decisions, and queries.

The reusable capability API is renamed consistently:

| Previous name | New name |
| --- | --- |
| `domain_decision` | `domain_policy` |
| `DecisionDescriptor` | `PolicyDescriptor` |
| `DecisionId` | `PolicyId` |
| `DecisionOutcome` | `PolicyOutcome` |
| `DecisionOutcomeType` | `PolicyOutcomeType` |
| `DecisionOutcomeDescriptor` | `PolicyOutcomeDescriptor` |
| `domain_decision_test` | `domain_policy_test` |
| `DomainTestSubject::Decision` | `DomainTestSubject::Policy` |
| `decision.rs` | `policy.rs` |
| compiled-model `decisions` | compiled-model `policies` |

Semantic IDs are unchanged. Capability directories keep their business names;
only their contract file and Rust vocabulary change. The typed filesystem
recognizes `domain_policy` in `policy.rs`, with the matching implementation in
`evaluate.rs` and any closed `PolicyOutcome` vocabulary in `outcome.rs`.

Domain-test discovery metadata advances to V2 and emits `"kind": "policy"`.
The compiled domain model's empty, reserved capability collection is named
`policies` rather than `decisions`.

Runtime and Tracer APIs that describe command handling remain unchanged,
including `CommandOutcome`, `CommandResult`, `SimulationDecision`, and
`CompletedDecision`.

## Consequences

The rename is an intentional breaking source and model change. Rostfrei is
still pre-1.0, so no deprecated aliases are introduced; aliases would preserve
the ambiguity and could not bridge filesystem roles or serialized domain-test
subjects cleanly.

Documentation must qualify the two concepts consistently: a command produces
a Decision, while actions, command decisions, and queries may consult Domain
Policies. Historical ADR bodies remain unchanged and carry supersession notes
where their old Decision vocabulary refers to reusable rules.
