---
title: Domain Testing
kind: handbook
---

# Domain Testing

Domain Tests are ordinary Rust tests explicitly linked to one modeled Action, Decision, Invariant, or Lifecycle. The link uses the subject's typed compiler reference and stable domain ID.

## Action Tests

```rust
#[domain_action_test(
    <RentalFleetAggregate as RentalFleetActionContract>::RENT_BICYCLE
)]
fn available_bicycle_can_be_rented() {
    verify_bicycle_rental();
}
```

## Decision Tests

```rust
#[domain_decision_test(
    RentalFleetAggregate::ASSESS_RENTAL_ELIGIBILITY
)]
fn maintenance_blocks_rental() {
    verify_rental_eligibility();
}
```

Decision tests should exercise the returned `DecisionOutcome` variants directly.
The test attribute links the test to the Decision as a whole; it does not label a
variant as accepted or denied and does not infer which outcomes the test covers.

## Invariant Tests

```rust
#[domain_invariant_test(<Product as InventoryBounds>::STOCK_NONNEGATIVE)]
fn negative_stock_is_rejected() {
    verify_negative_stock_rejection();
}
```

## Lifecycle Tests

```rust
#[domain_lifecycle_test(TaskLifecycle)]
fn completed_tasks_are_terminal() {
    verify_completed_state_is_terminal();
}
```

The Domain Test attribute owns the built-in `#[test]` attribute. Do not add `#[test]` to the same function. A test accepts no parameters or generics and cannot be async, const, unsafe, extern, or variadic.

Each Domain Test links to exactly one primary subject. An Action test does not automatically count as coverage for Decisions or Invariants called by that Action. Directly test and tag each subject whose behavior is being specified.

## Typed Links

Action and Invariant tests use an owner-qualified generated trait reference:

```text
<Owner as ContractTrait>::STABLE_REFERENCE
```

Decision tests retain readable owner-associated attribute syntax:

```text
Owner::STABLE_REFERENCE
```

The Decision macro generates a doc-hidden `DecisionReference<Group>` anchor for
each stable Decision ID. `domain_decision_test` resolves the readable syntax to
that group-typed anchor and requires `Group` to be attached to exactly `Owner`.
The syntax is for the test attribute; ordinary application calls continue to use
the authored inherent function name.

References are derived from stable subject IDs. Removing the subject, using the
wrong owner, naming an unknown reference, attaching the group to a different
owner, or testing a Decision whose exact group is unattached causes the test
target to fail compilation. This remains exact even when one owner has multiple
Decision groups.

Lifecycle tests use the lifecycle type. The type must implement `EntityLifecycleType`.

## Metadata

Each tagged test generates an ignored metadata companion. Ordinary `cargo test` runs the authored test and skips the companion.

Metadata can be extracted without executing authored Domain Tests:

```sh
cargo test --workspace __domain_test_metadata_ -- --ignored --show-output --test-threads=1
```

Each companion emits one line beginning with:

```text
ROSTFREI_DOMAIN_TEST_METADATA_V1
```

The remainder of the line is compact JSON containing the package, test target, test path, source location, subject kind, and complete stable subject ID. Tooling can join that ID to the compiled domain model.

The metadata records declared test linkage. They do not prove runtime branch or code coverage.

## Lifecycle Boundary

Current lifecycle compiler support is metadata-only. Lifecycle Domain Tests can assert initial states, declared transitions, omitted transitions, self-transitions, terminal states, and stable Action IDs. They do not test compiler-generated runtime admission or transition execution because the compiler does not currently generate that behavior.
