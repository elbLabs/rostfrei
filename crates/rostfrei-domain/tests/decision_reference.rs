#![allow(dead_code, private_bounds, private_interfaces, clippy::clone_on_copy)]

use domain::{
    AggregateId, BoundedContext, BoundedContextId, DecisionId, DecisionOwnerId, DecisionOwnerType,
    DecisionReference, DomainService, DomainServiceType, ValueObject, domain_decisions,
};
use std::{collections::HashSet, fmt::Debug, hash::Hash, mem::size_of};

#[derive(BoundedContext)]
#[domain(id = "reference-context", label = "Reference context")]
struct ReferenceContext;

#[derive(ValueObject)]
#[domain(id = "reference-input", label = "Reference input", owner = ReferenceContext)]
struct ReferenceInput(bool);

#[derive(ValueObject)]
#[domain(id = "reference-output", label = "Reference output", owner = ReferenceContext)]
struct ReferenceOutput(bool);

#[domain_decisions(domain_service)]
pub trait GeneratedReferenceDecisions {
    #[decision(id = "dispatch-request", label = "Dispatch request")]
    fn decide(input: ReferenceInput) -> ReferenceOutput;
}

#[derive(DomainService)]
#[domain(
    id = "reference-service",
    label = "Reference service",
    context = ReferenceContext,
    decisions = [GeneratedReferenceDecisions]
)]
struct ReferenceService;

impl GeneratedReferenceDecisions for ReferenceService {
    fn decide(input: ReferenceInput) -> ReferenceOutput {
        ReferenceOutput(input.0)
    }
}

const GENERATED_REFERENCE: DecisionReference<ReferenceService> =
    <ReferenceService as GeneratedReferenceDecisions>::__DOMAIN_DECISION_REFERENCE_DISPATCH_REQUEST;

struct PrimaryOwner;

impl DecisionOwnerType for PrimaryOwner {
    const DECISION_OWNER_ID: DecisionOwnerId = DecisionOwnerId::Aggregate(AggregateId {
        context: BoundedContextId("references"),
        local: "primary-owner",
    });
}

struct SecondaryOwner;

impl DecisionOwnerType for SecondaryOwner {
    const DECISION_OWNER_ID: DecisionOwnerId = DecisionOwnerId::Aggregate(AggregateId {
        context: BoundedContextId("references"),
        local: "secondary-owner",
    });
}

const PRIMARY_REFERENCE: DecisionReference<PrimaryOwner> =
    DecisionReference::__from_local("publish");
const PRIMARY_ID: DecisionId = PRIMARY_REFERENCE.id();
const PRIMARY_LOCAL_ID: &str = PRIMARY_REFERENCE.local_id();
const SECONDARY_REFERENCE: DecisionReference<SecondaryOwner> =
    DecisionReference::__from_local("publish");

fn assert_reference_traits<T: Copy + Clone + Debug + Eq + Hash>() {}

#[test]
fn generated_reference_matches_attached_decision_descriptor() {
    assert_eq!(
        GENERATED_REFERENCE.id(),
        <ReferenceService as DomainServiceType>::DECISION_CONTRACTS[0][0].id
    );
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
fn preserves_owner_type_and_local_value_behavior() {
    let duplicate = DecisionReference::<PrimaryOwner>::__from_local("publish");
    let different = DecisionReference::<PrimaryOwner>::__from_local("archive");

    assert_eq!(PRIMARY_REFERENCE, duplicate);
    assert_ne!(PRIMARY_REFERENCE, different);
    assert_eq!(PRIMARY_REFERENCE.local_id(), SECONDARY_REFERENCE.local_id());
    assert_ne!(PRIMARY_REFERENCE.id(), SECONDARY_REFERENCE.id());
    assert_eq!(
        size_of::<DecisionReference<PrimaryOwner>>(),
        size_of::<&'static str>()
    );
}

#[test]
#[allow(clippy::clone_on_copy)]
fn implements_value_traits_without_owner_trait_bounds() {
    assert_reference_traits::<DecisionReference<PrimaryOwner>>();

    let copied = PRIMARY_REFERENCE;
    let cloned = PRIMARY_REFERENCE.clone();
    let mut references = HashSet::new();

    references.insert(PRIMARY_REFERENCE);
    references.insert(copied);
    references.insert(cloned);

    assert_eq!(references.len(), 1);
    assert_eq!(
        format!("{PRIMARY_REFERENCE:?}"),
        "DecisionReference { id: DecisionId { owner: Aggregate(AggregateId { context: BoundedContextId(\"references\"), local: \"primary-owner\" }), local: \"publish\" } }"
    );
}
