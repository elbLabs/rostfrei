#![allow(non_snake_case, clippy::clone_on_copy)]

use domain::{InvariantId, InvariantReference, InvariantViolation, ValueObject, domain_invariants};
use std::{collections::HashSet, fmt::Debug, hash::Hash, mem::size_of};

#[domain_invariants]
trait GeneratedReferenceInvariants {
    #[invariant(id = "valid-value", label = "Value is valid")]
    fn validate(candidate: &ReferenceValue) -> Option<InvariantViolation>;
}

#[derive(ValueObject)]
#[domain(id = "reference-value", label = "Reference value")]
struct ReferenceValue(bool);

impl GeneratedReferenceInvariants for ReferenceValue {
    fn validate(candidate: &Self) -> Option<InvariantViolation> {
        (!candidate.0).then(|| InvariantViolation::new("value", "must be valid"))
    }
}

const GENERATED_REFERENCE: InvariantReference =
    <ReferenceValue as GeneratedReferenceInvariants>::__DOMAIN_INVARIANT_REFERENCE_VALID_VALUE;

const PRIMARY_REFERENCE: InvariantReference = InvariantReference::__from_local("publish");
const PRIMARY_ID: InvariantId = PRIMARY_REFERENCE.id();
const PRIMARY_LOCAL_ID: &str = PRIMARY_REFERENCE.local_id();

const fn assert_reference_traits<T: Copy + Clone + Debug + Eq + Hash>() {}

#[test]
fn generated_reference_matches_contract_descriptor() {
    assert_eq!(
        GENERATED_REFERENCE.id(),
        <ReferenceValue as GeneratedReferenceInvariants>::__DOMAIN_INVARIANTS[0].id
    );
    assert_eq!(
        <ReferenceValue as GeneratedReferenceInvariants>::validate(&ReferenceValue(true)),
        None
    );
}

#[test]
fn constructs_and_accesses_references_in_const_context() {
    assert_eq!(PRIMARY_LOCAL_ID, "publish");
    assert_eq!(PRIMARY_ID, InvariantId("publish"));
}

#[test]
fn preserves_local_value_behavior() {
    let duplicate = InvariantReference::__from_local("publish");
    let different = InvariantReference::__from_local("archive");

    assert_eq!(PRIMARY_REFERENCE, duplicate);
    assert_ne!(PRIMARY_REFERENCE, different);
    assert_eq!(size_of::<InvariantReference>(), size_of::<&'static str>());
}

#[test]
#[allow(clippy::clone_on_copy)]
fn implements_value_traits() {
    assert_reference_traits::<InvariantReference>();

    let copied = PRIMARY_REFERENCE;
    let cloned = PRIMARY_REFERENCE.clone();
    let mut references = HashSet::new();

    references.insert(PRIMARY_REFERENCE);
    references.insert(copied);
    references.insert(cloned);

    assert_eq!(references.len(), 1);
    assert_eq!(
        format!("{PRIMARY_REFERENCE:?}"),
        "InvariantReference { id: InvariantId(\"publish\") }"
    );
}
