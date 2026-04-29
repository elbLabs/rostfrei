#![allow(dead_code, clippy::clone_on_copy)]

use domain::DecisionOutcome;
use domain::{
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DecisionDescriptor,
    DecisionGroupType, DecisionId, DecisionOwnerId, DecisionOwnerType, DecisionReference,
    DomainIdentity, Entity, domain_decisions,
};
use std::{collections::HashSet, fmt::Debug, hash::Hash, mem::size_of};

struct ReferenceDecisions;

#[derive(BoundedContext)]
#[domain(id = "reference-context", label = "Reference context")]
struct ReferenceContext;

#[derive(DomainIdentity)]
struct ReferenceIdentity(u64);

#[derive(Aggregate)]
#[domain(id = "reference-aggregate", label = "Reference aggregate")]
struct ReferenceAggregate;

impl domain::AggregateDefinition for ReferenceAggregate {
    type Context = ReferenceContext;
    type Root = ReferenceRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(Entity)]
#[domain(id = "reference-root", label = "Reference root")]
struct ReferenceRoot {
    #[domain(identity)]
    id: ReferenceIdentity,
}

impl domain::EntityDefinition for ReferenceRoot {
    type Owner = ReferenceAggregate;
    type Identity = ReferenceIdentity;
}

#[derive(DecisionOutcome)]
enum ReferenceOutcome {
    #[outcome(id = "accepted", label = "Accepted")]
    Accepted(bool),
    #[outcome(id = "rejected", label = "Rejected")]
    Rejected,
}

#[domain_decisions(aggregate, group = ReferenceDecisions)]
impl ReferenceAggregate {
    #[decision(id = "dispatch-request", label = "Dispatch request")]
    const fn decide(input: bool) -> ReferenceOutcome {
        if input {
            ReferenceOutcome::Accepted(true)
        } else {
            ReferenceOutcome::Rejected
        }
    }
}

const GENERATED_REFERENCE: DecisionReference<ReferenceDecisions> =
    ReferenceAggregate::__DOMAIN_DECISION_REFERENCE_DISPATCH_REQUEST;

struct PrimaryOwner;

impl DecisionOwnerType for PrimaryOwner {
    const DECISION_OWNER_ID: DecisionOwnerId = DecisionOwnerId::Aggregate(AggregateId {
        context: BoundedContextId("references"),
        local: "primary-owner",
    });
}

struct PrimaryGroup;

impl DecisionGroupType for PrimaryGroup {
    type Owner = PrimaryOwner;

    const DECISIONS: &'static [DecisionDescriptor] = &[];
}

struct SecondaryOwner;

impl DecisionOwnerType for SecondaryOwner {
    const DECISION_OWNER_ID: DecisionOwnerId = DecisionOwnerId::Aggregate(AggregateId {
        context: BoundedContextId("references"),
        local: "secondary-owner",
    });
}

struct SecondaryGroup;

impl DecisionGroupType for SecondaryGroup {
    type Owner = SecondaryOwner;

    const DECISIONS: &'static [DecisionDescriptor] = &[];
}

const PRIMARY_REFERENCE: DecisionReference<PrimaryGroup> =
    DecisionReference::__from_local("publish");
const PRIMARY_ID: DecisionId = PRIMARY_REFERENCE.id();
const PRIMARY_LOCAL_ID: &str = PRIMARY_REFERENCE.local_id();
const SECONDARY_REFERENCE: DecisionReference<SecondaryGroup> =
    DecisionReference::__from_local("publish");

const fn assert_reference_traits<T: Copy + Clone + Debug + Eq + Hash>() {}

#[test]
fn generated_reference_matches_its_group_descriptor() {
    assert_eq!(
        GENERATED_REFERENCE.id(),
        <ReferenceDecisions as DecisionGroupType>::DECISIONS[0].id
    );
    assert_eq!(GENERATED_REFERENCE.local_id(), "dispatch-request");
}

#[test]
fn constructs_and_accesses_references_in_const_context() {
    assert_eq!(PRIMARY_LOCAL_ID, "publish");
    assert_eq!(
        PRIMARY_ID,
        DecisionId {
            owner: PrimaryOwner::DECISION_OWNER_ID,
            local: "publish",
        }
    );
}

#[test]
fn preserves_group_owner_type_and_local_value_behavior() {
    let duplicate = DecisionReference::<PrimaryGroup>::__from_local("publish");
    let different = DecisionReference::<PrimaryGroup>::__from_local("archive");

    assert_eq!(PRIMARY_REFERENCE, duplicate);
    assert_ne!(PRIMARY_REFERENCE, different);
    assert_eq!(PRIMARY_REFERENCE.local_id(), SECONDARY_REFERENCE.local_id());
    assert_ne!(PRIMARY_REFERENCE.id(), SECONDARY_REFERENCE.id());
    assert_eq!(
        size_of::<DecisionReference<PrimaryGroup>>(),
        size_of::<&'static str>()
    );
}

#[test]
fn implements_value_traits_without_group_trait_bounds() {
    assert_reference_traits::<DecisionReference<PrimaryGroup>>();
    let mut references = HashSet::new();
    references.insert(PRIMARY_REFERENCE);
    references.insert(PRIMARY_REFERENCE.clone());
    assert_eq!(references.len(), 1);
}
