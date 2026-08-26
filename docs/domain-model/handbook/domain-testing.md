---
title: Domain Testing
kind: handbook
---

# Domain Testing

Domain Tests are ordinary Rust tests explicitly linked to one modeled Action, Decision, Invariant, or Lifecycle. The link uses the subject's typed compiler reference and stable domain ID.

## Action Tests

```rust
#[domain_action_test(
    <RentalFleetAggregate as RentalFleetActions>::RENT_BICYCLE
)]
fn available_bicycle_can_be_rented() {
    verify_bicycle_rental();
}
```

## Decision Tests

```rust
#[domain_decision_test(
    <RentalFleetAggregate as RentalEligibilityDecisions>
        ::ASSESS_RENTAL_ELIGIBILITY
)]
fn maintenance_blocks_rental() {
    verify_rental_eligibility();
}
```

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

Action, Decision, and Invariant tests use an owner-qualified generated reference:

```text
<Owner as ContractTrait>::STABLE_REFERENCE
```

The reference is derived from the subject's stable ID. Removing the subject, using the wrong owner, or naming an unknown reference causes the test target to fail compilation.

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
