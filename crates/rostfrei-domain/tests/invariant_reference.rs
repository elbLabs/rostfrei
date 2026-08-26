#![allow(non_snake_case, clippy::clone_on_copy)]

use domain::{
    AggregateId, BoundedContext, BoundedContextId, InvariantId, InvariantOwnerId,
    InvariantOwnerType, InvariantReference, InvariantViolation, ValueObject, ValueObjectType,
    domain_invariants,
};
use std::{collections::HashSet, fmt::Debug, hash::Hash, mem::size_of};

#[derive(BoundedContext)]
#[domain(id = "reference-context", label = "Reference context")]
struct ReferenceContext;

#[domain_invariants(value_object)]
trait GeneratedReferenceInvariants {
    #[invariant(id = "valid-value", label = "Value is valid")]
    fn validate(candidate: &<Self as InvariantOwnerType>::Candidate) -> Option<InvariantViolation>;
}

#[derive(ValueObject)]
#[domain(
    id = "reference-value",
    label = "Reference value",
    owner = ReferenceContext,
    invariants = [GeneratedReferenceInvariants]
)]
struct ReferenceValue(bool);

impl GeneratedReferenceInvariants for ReferenceValue {
    fn validate(candidate: &ReferenceValue) -> Option<InvariantViolation> {
        (!candidate.0).then(|| InvariantViolation::new("value", "must be valid"))
    }
}

const GENERATED_REFERENCE: InvariantReference<ReferenceValue> =
    <ReferenceValue as GeneratedReferenceInvariants>::__DOMAIN_INVARIANT_REFERENCE_VALID_VALUE;

struct PrimaryOwner;

impl InvariantOwnerType for PrimaryOwner {
    type Candidate = ();

    const INVARIANT_OWNER_ID: InvariantOwnerId = InvariantOwnerId::Aggregate(AggregateId {
        context: BoundedContextId("references"),
        local: "primary-owner",
    });
}

struct SecondaryOwner;

impl InvariantOwnerType for SecondaryOwner {
    type Candidate = ();

    const INVARIANT_OWNER_ID: InvariantOwnerId = InvariantOwnerId::Aggregate(AggregateId {
        context: BoundedContextId("references"),
        local: "secondary-owner",
    });
}

const PRIMARY_REFERENCE: InvariantReference<PrimaryOwner> =
    InvariantReference::__from_local("publish");
const PRIMARY_ID: InvariantId = PRIMARY_REFERENCE.id();
const PRIMARY_LOCAL_ID: &str = PRIMARY_REFERENCE.local_id();
const SECONDARY_REFERENCE: InvariantReference<SecondaryOwner> =
    InvariantReference::__from_local("publish");

fn assert_reference_traits<T: Copy + Clone + Debug + Eq + Hash>() {}

#[test]
fn generated_reference_matches_attached_invariant_descriptor() {
    assert_eq!(
        GENERATED_REFERENCE.id(),
        <ReferenceValue as ValueObjectType>::INVARIANT_CONTRACTS[0][0].id
    );
}

#[test]
fn constructs_and_accesses_references_in_const_context() {
    assert_eq!(PRIMARY_LOCAL_ID, "publish");
    assert_eq!(
        PRIMARY_ID,
        InvariantId {
            owner: PrimaryOwner::INVARIANT_OWNER_ID,
            local: "publish",
        }
    );
}

#[test]
fn preserves_owner_type_and_local_value_behavior() {
    let duplicate = InvariantReference::<PrimaryOwner>::__from_local("publish");
    let different = InvariantReference::<PrimaryOwner>::__from_local("archive");

    assert_eq!(PRIMARY_REFERENCE, duplicate);
    assert_ne!(PRIMARY_REFERENCE, different);
    assert_eq!(PRIMARY_REFERENCE.local_id(), SECONDARY_REFERENCE.local_id());
    assert_ne!(PRIMARY_REFERENCE.id(), SECONDARY_REFERENCE.id());
    assert_eq!(
        size_of::<InvariantReference<PrimaryOwner>>(),
        size_of::<&'static str>()
    );
}

#[test]
#[allow(clippy::clone_on_copy)]
fn implements_value_traits_without_owner_trait_bounds() {
    assert_reference_traits::<InvariantReference<PrimaryOwner>>();

    let copied = PRIMARY_REFERENCE;
    let cloned = PRIMARY_REFERENCE.clone();
    let mut references = HashSet::new();

    references.insert(PRIMARY_REFERENCE);
    references.insert(copied);
    references.insert(cloned);

    assert_eq!(references.len(), 1);
    assert_eq!(
        format!("{PRIMARY_REFERENCE:?}"),
        "InvariantReference { id: InvariantId { owner: Aggregate(AggregateId { context: BoundedContextId(\"references\"), local: \"primary-owner\" }), local: \"publish\" } }"
    );
}
