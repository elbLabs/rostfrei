use rostfrei_domain::{
    ActionId, ActionOwnerId, ActionOwnerType, ActionReference, AggregateId, BoundedContext,
    BoundedContextId, DomainService, DomainServiceType, domain_actions,
};
use std::{collections::HashSet, fmt::Debug, hash::Hash, mem::size_of};

#[derive(BoundedContext)]
#[domain(id = "reference-context", label = "Reference context")]
struct ReferenceContext;

#[domain_actions(domain_service)]
pub trait GeneratedReferenceActions {
    #[action(id = "dispatch-request", label = "Dispatch request")]
    fn execute();
}

#[derive(DomainService)]
#[domain(
    id = "reference-service",
    label = "Reference service",
    context = ReferenceContext,
    actions = [GeneratedReferenceActions]
)]
struct ReferenceService;

impl GeneratedReferenceActions for ReferenceService {
    fn execute() {}
}

const GENERATED_REFERENCE: ActionReference<ReferenceService> =
    <ReferenceService as GeneratedReferenceActions>::__DOMAIN_ACTION_REFERENCE_DISPATCH_REQUEST;

struct PrimaryOwner;

impl ActionOwnerType for PrimaryOwner {
    const ACTION_OWNER_ID: ActionOwnerId = ActionOwnerId::Aggregate(AggregateId {
        context: BoundedContextId("references"),
        local: "primary-owner",
    });
}

struct SecondaryOwner;

impl ActionOwnerType for SecondaryOwner {
    const ACTION_OWNER_ID: ActionOwnerId = ActionOwnerId::Aggregate(AggregateId {
        context: BoundedContextId("references"),
        local: "secondary-owner",
    });
}

const PRIMARY_REFERENCE: ActionReference<PrimaryOwner> = ActionReference::__from_local("publish");
const PRIMARY_ID: ActionId = PRIMARY_REFERENCE.id();
const PRIMARY_LOCAL_ID: &str = PRIMARY_REFERENCE.local_id();
const SECONDARY_REFERENCE: ActionReference<SecondaryOwner> =
    ActionReference::__from_local("publish");

fn assert_reference_traits<T: Copy + Clone + Debug + Eq + Hash>() {}

#[test]
fn generated_reference_matches_attached_action_descriptor() {
    assert_eq!(
        GENERATED_REFERENCE.id(),
        <ReferenceService as DomainServiceType>::ACTION_CONTRACTS[0][0].id
    );
}

#[test]
fn constructs_and_accesses_references_in_const_context() {
    assert_eq!(PRIMARY_LOCAL_ID, "publish");
    assert_eq!(
        PRIMARY_ID,
        ActionId {
            owner: PrimaryOwner::ACTION_OWNER_ID,
            local: "publish",
        }
    );
}

#[test]
fn preserves_owner_type_and_local_value_behavior() {
    let duplicate = ActionReference::<PrimaryOwner>::__from_local("publish");
    let different = ActionReference::<PrimaryOwner>::__from_local("archive");

    assert_eq!(PRIMARY_REFERENCE, duplicate);
    assert_ne!(PRIMARY_REFERENCE, different);
    assert_eq!(PRIMARY_REFERENCE.local_id(), SECONDARY_REFERENCE.local_id());
    assert_ne!(PRIMARY_REFERENCE.id(), SECONDARY_REFERENCE.id());
    assert_eq!(
        size_of::<ActionReference<PrimaryOwner>>(),
        size_of::<&'static str>()
    );
}

#[test]
#[allow(clippy::clone_on_copy)]
fn implements_value_traits_without_owner_trait_bounds() {
    assert_reference_traits::<ActionReference<PrimaryOwner>>();

    let copied = PRIMARY_REFERENCE;
    let cloned = PRIMARY_REFERENCE.clone();
    let mut references = HashSet::new();

    references.insert(PRIMARY_REFERENCE);
    references.insert(copied);
    references.insert(cloned);

    assert_eq!(references.len(), 1);
    assert_eq!(
        format!("{PRIMARY_REFERENCE:?}"),
        "ActionReference { id: ActionId { owner: Aggregate(AggregateId { context: BoundedContextId(\"references\"), local: \"primary-owner\" }), local: \"publish\" } }"
    );
}
