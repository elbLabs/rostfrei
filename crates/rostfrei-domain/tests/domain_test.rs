#![allow(dead_code, non_snake_case, private_bounds, private_interfaces)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, EntityLifecycle, EntityLifecycleType,
    InvariantOwnerType, InvariantViolation, ValueObject, domain_action_test, domain_actions,
    domain_decision_test, domain_decisions, domain_invariant_test, domain_invariants,
    domain_lifecycle_test,
};

#[derive(BoundedContext)]
#[domain(id = "testing", label = "Testing")]
struct Testing;

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(id = "decision-input", label = "Decision input", owner = TestAggregate)]
struct DecisionInput(bool);

#[derive(ValueObject, Debug, Eq, PartialEq)]
#[domain(id = "decision-output", label = "Decision output", owner = TestAggregate)]
struct DecisionOutput(bool);

#[domain_actions(aggregate)]
pub trait AggregateActions {
    #[action(id = "mark", label = "Mark")]
    fn mark(root: &mut TestRoot, input: bool);
}

#[domain_actions(entity)]
trait RootActions {
    #[action(id = "activate", label = "Activate")]
    fn activate(&mut self);
}

#[domain_invariants(aggregate)]
trait AggregateInvariants {
    #[invariant(id = "marked", label = "Marked")]
    fn marked(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
}

#[derive(EntityLifecycle)]
#[domain(
    id = "test-lifecycle",
    label = "Test lifecycle",
    owner = TestRoot,
    initial = Draft,
)]
enum TestLifecycle {
    #[domain(id = "draft", label = "Draft")]
    #[transition(action = RootActions::ACTIVATE, to = Active)]
    Draft,
    #[domain(id = "active", label = "Active")]
    Active,
}

#[derive(DomainIdentity)]
#[domain(owner = TestRoot)]
struct TestId(u64);

#[derive(Entity)]
#[domain(
    id = "test-root",
    label = "Test root",
    owner = TestAggregate,
    actions = [RootActions],
    lifecycle = TestLifecycle,
)]
struct TestRoot {
    #[domain(identity)]
    id: TestId,
    marked: bool,
    active: bool,
}

#[derive(Aggregate)]
#[domain(
    id = "test-aggregate",
    label = "Test aggregate",
    context = Testing,
    root = TestRoot,
    actions = [AggregateActions],
    decisions,
    invariants = [AggregateInvariants],
)]
struct TestAggregate;

#[domain_decisions(aggregate)]
impl TestAggregate {
    #[decision(id = "accept", label = "Accept")]
    const fn accept(input: DecisionInput) -> Result<DecisionOutput, DecisionOutput> {
        if input.0 {
            Ok(DecisionOutput(true))
        } else {
            Err(DecisionOutput(false))
        }
    }
}

impl AggregateActions for TestAggregate {
    fn mark(root: &mut TestRoot, input: bool) {
        root.marked = input;
    }
}

impl RootActions for TestRoot {
    fn activate(&mut self) {
        self.active = true;
    }
}

impl AggregateInvariants for TestAggregate {
    fn marked(candidate: &TestRoot) -> Option<InvariantViolation> {
        (!candidate.marked).then(|| InvariantViolation::new("marked", "must be marked"))
    }
}

const fn root() -> TestRoot {
    TestRoot {
        id: TestId(1),
        marked: false,
        active: false,
    }
}

#[domain_lifecycle_test(MissingLifecycle)]
#[cfg(any())]
fn cfg_disabled_domain_test_does_not_resolve_its_subject() {}

#[domain_lifecycle_test(MissingLifecycle)]
#[cfg_attr(all(), cfg(any()))]
fn nested_cfg_disabled_domain_test_does_not_resolve_its_subject() {}

#[domain_action_test(<TestAggregate as AggregateActions>::MARK)]
fn action_tests_keep_the_authored_body() {
    let mut root = root();
    TestAggregate::mark(&mut root, true);
    assert!(root.marked);
}

#[domain_action_test(<TestAggregate as AggregateActions>::MARK)]
fn case_distinct_test_name() {}

#[domain_action_test(<TestAggregate as AggregateActions>::MARK)]
fn CASE_DISTINCT_TEST_NAME() {}

#[domain_decision_test(TestAggregate::ACCEPT)]
fn decision_tests_keep_the_authored_body() {
    assert_eq!(
        TestAggregate::accept(DecisionInput(true)),
        Ok(DecisionOutput(true))
    );
}

#[domain_invariant_test(<TestAggregate as AggregateInvariants>::MARKED)]
fn invariant_tests_keep_the_authored_body() {
    assert_eq!(
        <TestAggregate as InvariantOwnerType>::validate_invariants(&root()),
        Err(vec![InvariantViolation::new("marked", "must be marked")])
    );
}

#[domain_lifecycle_test(TestLifecycle)]
fn lifecycle_tests_keep_the_authored_body() {
    let lifecycle = TestLifecycle::DESCRIPTOR;
    assert_eq!(lifecycle.initial.local, "draft");
    assert_eq!(lifecycle.transitions[0].target.local, "active");
}
