#![allow(dead_code, non_snake_case, private_bounds, private_interfaces)]

use domain::DecisionOutcome;
use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, EntityLifecycle, EntityLifecycleType,
    InvariantViolation, ValueObject, domain_action, domain_action_test, domain_decision_test,
    domain_decisions, domain_invariant_test, domain_invariants, domain_lifecycle_test,
};

#[derive(BoundedContext)]
#[domain(id = "testing", label = "Testing")]
struct Testing;

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(id = "decision-input", label = "Decision input")]
struct DecisionInput(bool);

#[derive(ValueObject, Debug, Eq, PartialEq)]
#[domain(id = "decision-output", label = "Decision output")]
struct DecisionOutput(bool);

#[derive(DecisionOutcome, Debug, Eq, PartialEq)]
enum TestDecisionOutcome {
    #[outcome(id = "accepted", label = "Accepted")]
    Accepted(DecisionOutput),
    #[outcome(id = "rejected", label = "Rejected")]
    Rejected,
}

struct TestDecisions;

#[domain_action(id = "mark", label = "Mark")]
pub trait MarkAction {
    fn mark(root: &mut TestRoot, input: bool);
}

#[domain_invariants]
trait AggregateInvariants {
    #[invariant(id = "marked", label = "Marked")]
    fn marked(candidate: &TestRoot) -> Option<InvariantViolation>;
}

#[derive(EntityLifecycle)]
#[domain(id = "test-lifecycle", label = "Test lifecycle")]
enum TestLifecycle {
    #[state(id = "draft", label = "Draft")]
    Draft,
    #[state(id = "active", label = "Active")]
    Active,
}

#[derive(DomainIdentity)]
struct TestId(u64);

#[derive(Entity)]
#[domain(id = "test-root", label = "Test root")]
struct TestRoot {
    #[domain(identity)]
    id: TestId,
    marked: bool,
    active: bool,
}

impl domain::EntityDefinition for TestRoot {
    type Owner = TestAggregate;
    type Identity = TestId;
}

#[derive(Aggregate)]
#[domain(id = "test-aggregate", label = "Test aggregate")]
struct TestAggregate;

impl domain::AggregateDefinition for TestAggregate {
    type Context = Testing;
    type Root = TestRoot;
    type Event = domain::NoDomainEvents;
}

impl domain::__private::AttachedDecisionGroup<TestDecisions> for TestAggregate {}

#[domain_decisions(aggregate, group = TestDecisions)]
impl TestAggregate {
    #[decision(id = "accept", label = "Accept")]
    const fn accept(input: DecisionInput) -> TestDecisionOutcome {
        if input.0 {
            TestDecisionOutcome::Accepted(DecisionOutput(true))
        } else {
            TestDecisionOutcome::Rejected
        }
    }
}

impl MarkAction for TestAggregate {
    fn mark(root: &mut TestRoot, input: bool) {
        root.marked = input;
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

#[domain_action_test(<TestAggregate as MarkAction>::DESCRIPTOR)]
fn action_tests_keep_the_authored_body() {
    let mut root = root();
    TestAggregate::mark(&mut root, true);
    assert!(root.marked);
}

#[domain_action_test(<TestAggregate as MarkAction>::DESCRIPTOR)]
fn case_distinct_test_name() {}

#[domain_action_test(<TestAggregate as MarkAction>::DESCRIPTOR)]
fn CASE_DISTINCT_TEST_NAME() {}

#[domain_decision_test(TestAggregate::ACCEPT)]
fn decision_tests_keep_the_authored_body() {
    assert_eq!(
        TestAggregate::accept(DecisionInput(true)),
        TestDecisionOutcome::Accepted(DecisionOutput(true))
    );
}

#[domain_invariant_test(<TestAggregate as AggregateInvariants>::MARKED)]
fn invariant_tests_keep_the_authored_body() {
    assert_eq!(
        <TestAggregate as AggregateInvariants>::marked(&root()),
        Some(InvariantViolation::new("marked", "must be marked"))
    );
}

#[domain_lifecycle_test(TestLifecycle)]
fn lifecycle_tests_keep_the_authored_body() {
    let lifecycle = TestLifecycle::DESCRIPTOR;
    assert_eq!(lifecycle.states[0].id.local, "draft");
    assert_eq!(lifecycle.states[1].id.local, "active");
}
