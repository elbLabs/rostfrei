#![allow(dead_code, non_snake_case, private_bounds, private_interfaces)]

use domain::PolicyOutcome;
use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, EntityLifecycle, EntityLifecycleType,
    InvariantViolation, ValueObject, domain_action, domain_action_test, domain_invariant,
    domain_invariant_test, domain_lifecycle_test, domain_policy, domain_policy_test,
};

#[derive(BoundedContext)]
#[domain(id = "testing", label = "Testing")]
struct Testing;

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(id = "decision-input", label = "Policy input")]
struct PolicyInput(bool);

#[derive(ValueObject, Debug, Eq, PartialEq)]
#[domain(id = "decision-output", label = "Policy output")]
struct PolicyOutput(bool);

#[derive(PolicyOutcome, Debug, Eq, PartialEq)]
enum TestPolicyOutcome {
    #[outcome(id = "accepted", label = "Accepted")]
    Accepted(PolicyOutput),
    #[outcome(id = "rejected", label = "Rejected")]
    Rejected,
}

#[domain_action(id = "mark", label = "Mark")]
pub trait MarkAction {
    fn mark(root: &mut TestRoot, input: bool);
}

#[domain_invariant(id = "marked", label = "Marked")]
trait MarkedInvariant {
    fn marked(candidate: &TestRoot) -> Option<InvariantViolation>;
}

#[domain_policy(id = "accept", label = "Accept")]
trait AcceptPolicy {
    fn accept(input: PolicyInput) -> TestPolicyOutcome;
}

#[derive(EntityLifecycle, Clone, Copy, Eq, PartialEq)]
#[domain(id = "test-lifecycle", label = "Test lifecycle")]
#[lifecycle(initial = Draft)]
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
    id: TestId,
    marked: bool,
    active: bool,
}

impl domain::EntityDefinition for TestRoot {
    type Owner = TestAggregate;
    type Identity = TestId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Aggregate)]
#[domain(id = "test-aggregate", label = "Test aggregate")]
struct TestAggregate;

impl domain::AggregateDefinition for TestAggregate {
    type Context = Testing;
    type Root = TestRoot;
    type Event = domain::NoDomainEvents;
}

impl AcceptPolicy for TestAggregate {
    fn accept(input: PolicyInput) -> TestPolicyOutcome {
        if input.0 {
            TestPolicyOutcome::Accepted(PolicyOutput(true))
        } else {
            TestPolicyOutcome::Rejected
        }
    }
}

impl MarkAction for TestAggregate {
    fn mark(root: &mut TestRoot, input: bool) {
        root.marked = input;
    }
}

impl MarkedInvariant for TestAggregate {
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

#[domain_policy_test(<TestAggregate as AcceptPolicy>::DESCRIPTOR)]
fn policy_tests_keep_the_authored_body() {
    assert_eq!(
        TestAggregate::accept(PolicyInput(true)),
        TestPolicyOutcome::Accepted(PolicyOutput(true))
    );
}

#[domain_invariant_test(<TestAggregate as MarkedInvariant>::DESCRIPTOR)]
fn invariant_tests_keep_the_authored_body() {
    assert_eq!(
        <TestAggregate as MarkedInvariant>::marked(&root()),
        Some(InvariantViolation::new("marked", "must be marked"))
    );
}

#[domain_lifecycle_test(TestLifecycle)]
fn lifecycle_tests_keep_the_authored_body() {
    let lifecycle = TestLifecycle::DESCRIPTOR;
    assert_eq!(lifecycle.states[0].id.local, "draft");
    assert_eq!(lifecycle.states[1].id.local, "active");
}
rostfrei_domain_macros::__install_test_macro_support!();
