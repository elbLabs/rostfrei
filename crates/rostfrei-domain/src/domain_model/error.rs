use std::{error::Error, fmt};

use crate::{
    ActionId, ActionOwnerId, AggregateId, DecisionId, DecisionOutcomeId, DecisionOwnerId,
    DomainCommandId, DomainErrorId, DomainEventId, DomainIdentityId, EntityId, EntityLifecycleId,
    EntityLifecycleStateId, InvariantId, InvariantOwnerId, QueryId, ScalarType, ValueObjectId,
};

/// A domain-model descriptor reference involved in an inventory validation failure.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainModelReference {
    DomainIdentity(Box<DomainIdentityId>),
    DomainEvent(Box<DomainEventId>),
    DomainError(Box<DomainErrorId>),
    ValueObject(Box<ValueObjectId>),
    Entity(Box<EntityId>),
    Aggregate(Box<AggregateId>),
}

/// An error encountered while constructing or validating a domain model.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainModelError {
    UnregisteredActionExtensionOwner {
        owner: Box<ActionOwnerId>,
    },
    EmptyActionExtension,
    ActionDescriptorOwnerMismatch {
        id: Box<ActionId>,
    },
    DuplicateActionId {
        id: Box<ActionId>,
    },
    ActionReferenceInventoryViolation {
        action_id: Box<ActionId>,
        reference: DomainModelReference,
        location: String,
        inventory_key: &'static str,
    },
    ActionRaisedEventOwnerNotAggregate {
        action_id: Box<ActionId>,
    },
    ActionRaisedEventOwnerMismatch {
        action_id: Box<ActionId>,
        event_id: Box<DomainEventId>,
    },
    UnregisteredDecisionOwner {
        owner: Box<DecisionOwnerId>,
    },
    DecisionDescriptorOwnerMismatch {
        id: Box<DecisionId>,
    },
    DuplicateDecisionId {
        id: Box<DecisionId>,
    },
    DecisionWithoutOutcomes {
        decision_id: Box<DecisionId>,
    },
    DuplicateDecisionOutcomeId {
        id: Box<DecisionOutcomeId>,
    },
    DecisionReferenceInventoryViolation {
        decision_id: Box<DecisionId>,
        value_object_id: Box<ValueObjectId>,
        location: String,
    },
    FieldReferenceInventoryViolation {
        reference: DomainModelReference,
        location: String,
        inventory_key: &'static str,
    },
    UnregisteredInvariantOwner {
        owner: Box<InvariantOwnerId>,
    },
    InvariantDescriptorOwnerMismatch {
        id: Box<InvariantId>,
    },
    DuplicateInvariantId {
        id: Box<InvariantId>,
    },
    LifecycleExtensionOnlyAction {
        lifecycle_id: Box<EntityLifecycleId>,
        action_id: Box<ActionId>,
    },
    LifecycleMissingAttachedAction {
        lifecycle_id: Box<EntityLifecycleId>,
        action_id: Box<ActionId>,
    },
    LifecycleDescriptorOwnerMismatch {
        expected: Box<EntityId>,
        found: Box<EntityId>,
    },
    LifecycleWithoutStates {
        lifecycle_id: Box<EntityLifecycleId>,
    },
    DuplicateEntityLifecycleStateId {
        id: Box<EntityLifecycleStateId>,
    },
    DuplicateLifecycleTransitionKey {
        source: Box<EntityLifecycleStateId>,
        action: Box<ActionId>,
    },
    LifecycleStateOwnershipMismatch {
        location: &'static str,
        expected: Box<EntityLifecycleId>,
        found: Box<EntityLifecycleId>,
    },
    LifecycleStateNotDeclared {
        location: &'static str,
        id: Box<EntityLifecycleStateId>,
    },
    LifecycleTransitionActionOwnerMismatch {
        expected: Box<ActionOwnerId>,
        found: Box<ActionOwnerId>,
    },
    InvalidLifecycleLocalId {
        local: &'static str,
    },
    EmptyLifecycleLabel {
        label: &'static str,
    },
    InvalidLifecycleStateLocalId {
        local: &'static str,
    },
    EmptyLifecycleStateLabel {
        label: &'static str,
    },
    DomainIdentitySemanticScalarRepresentationMismatch {
        canonical: ScalarType,
        semantic: ScalarType,
    },
    DuplicateDomainIdentityId {
        id: Box<DomainIdentityId>,
    },
    DuplicateDomainEventId {
        id: Box<DomainEventId>,
    },
    DuplicateDomainCommandId {
        id: Box<DomainCommandId>,
    },
    DuplicateQueryId {
        id: Box<QueryId>,
    },
}

impl fmt::Display for DomainModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnregisteredActionExtensionOwner { owner } => {
                write!(formatter, "unregistered action extension owner: {owner:?}")
            }
            Self::EmptyActionExtension => write!(formatter, "action extension must not be empty"),
            Self::ActionDescriptorOwnerMismatch { id } => {
                write!(formatter, "action descriptor owner mismatch: {id:?}")
            }
            Self::DuplicateActionId { id } => write!(formatter, "duplicate ActionId: {id:?}"),
            Self::ActionReferenceInventoryViolation {
                action_id,
                reference,
                location,
                inventory_key,
            } => fmt_action_reference(formatter, action_id, reference, location, inventory_key),
            Self::ActionRaisedEventOwnerNotAggregate { action_id: id } => {
                fmt_raised_owner(formatter, id, None)
            }
            Self::ActionRaisedEventOwnerMismatch {
                action_id,
                event_id,
            } => fmt_raised_owner(formatter, action_id, Some(event_id)),
            Self::UnregisteredDecisionOwner { owner } => fmt_unregistered_owner(formatter, owner),
            Self::DecisionDescriptorOwnerMismatch { id } => fmt_decision_owner(formatter, id),
            Self::DuplicateDecisionId { id } => write!(formatter, "duplicate DecisionId: {id:?}"),
            Self::DecisionWithoutOutcomes { decision_id } => {
                fmt_empty_decision(formatter, decision_id)
            }
            Self::DuplicateDecisionOutcomeId { id } => fmt_duplicate_outcome(formatter, id),
            Self::DecisionReferenceInventoryViolation {
                decision_id,
                value_object_id,
                location,
            } => fmt_decision_reference(formatter, decision_id, value_object_id, location),
            Self::FieldReferenceInventoryViolation {
                reference,
                location,
                inventory_key,
            } => fmt_field_reference(formatter, reference, location, inventory_key),
            Self::UnregisteredInvariantOwner { owner } => fmt_unregistered_inv(formatter, owner),
            Self::InvariantDescriptorOwnerMismatch { id } => {
                write!(formatter, "invariant descriptor owner mismatch: {id:?}")
            }
            Self::DuplicateInvariantId { id } => fmt_duplicate(formatter, "InvariantId", id),
            Self::LifecycleExtensionOnlyAction {
                lifecycle_id,
                action_id,
            } => fmt_extension_only_action(formatter, lifecycle_id, action_id),
            Self::LifecycleMissingAttachedAction {
                lifecycle_id,
                action_id,
            } => fmt_missing_attached_action(formatter, lifecycle_id, action_id),
            Self::LifecycleDescriptorOwnerMismatch { expected, found } => {
                fmt_lifecycle_owner_mismatch(formatter, expected, found)
            }
            Self::LifecycleWithoutStates { lifecycle_id } => {
                fmt_lifecycle_without_states(formatter, lifecycle_id)
            }
            Self::DuplicateEntityLifecycleStateId { id } => {
                write!(formatter, "duplicate EntityLifecycleStateId: {id:?}")
            }
            Self::DuplicateLifecycleTransitionKey { source, action } => {
                fmt_duplicate_transition(formatter, source, action)
            }
            Self::LifecycleStateOwnershipMismatch {
                location,
                expected,
                found,
            } => fmt_state_ownership(formatter, location, expected, found),
            Self::LifecycleStateNotDeclared { location, id } => {
                fmt_state_not_declared(formatter, location, id)
            }
            Self::LifecycleTransitionActionOwnerMismatch { expected, found } => {
                fmt_lifecycle_action_owner_mismatch(formatter, expected, found)
            }
            Self::InvalidLifecycleLocalId { local } => {
                fmt_invalid_lifecycle_id(formatter, "", local)
            }
            Self::EmptyLifecycleLabel { label } => fmt_empty_lifecycle_label(formatter, "", label),
            Self::InvalidLifecycleStateLocalId { local } => {
                fmt_invalid_lifecycle_id(formatter, "state ", local)
            }
            Self::EmptyLifecycleStateLabel { label } => {
                fmt_empty_lifecycle_label(formatter, "state ", label)
            }
            Self::DomainIdentitySemanticScalarRepresentationMismatch {
                canonical,
                semantic,
            } => fmt_semantic_scalar_mismatch(formatter, *canonical, *semantic),
            Self::DuplicateDomainIdentityId { id } => {
                fmt_duplicate(formatter, "DomainIdentityId", id)
            }
            Self::DuplicateDomainEventId { id } => fmt_duplicate(formatter, "DomainEventId", id),
            Self::DuplicateDomainCommandId { id } => {
                fmt_duplicate(formatter, "DomainCommandId", id)
            }
            Self::DuplicateQueryId { id } => write!(formatter, "duplicate QueryId: {id:?}"),
        }
    }
}

impl Error for DomainModelError {}

fn fmt_duplicate(
    formatter: &mut fmt::Formatter<'_>,
    kind: &str,
    id: impl fmt::Debug,
) -> fmt::Result {
    write!(formatter, "duplicate {kind}: {id:?}")
}

fn fmt_unregistered_owner(
    formatter: &mut fmt::Formatter<'_>,
    owner: &DecisionOwnerId,
) -> fmt::Result {
    write!(formatter, "unregistered decision owner: {owner:?}")
}

fn fmt_duplicate_outcome(
    formatter: &mut fmt::Formatter<'_>,
    id: &DecisionOutcomeId,
) -> fmt::Result {
    fmt_duplicate(formatter, "DecisionOutcomeId", id)
}

fn fmt_unregistered_inv(
    formatter: &mut fmt::Formatter<'_>,
    owner: &InvariantOwnerId,
) -> fmt::Result {
    write!(formatter, "unregistered invariant owner: {owner:?}")
}

fn fmt_decision_owner(formatter: &mut fmt::Formatter<'_>, id: &DecisionId) -> fmt::Result {
    write!(formatter, "decision descriptor owner mismatch: {id:?}")
}

fn fmt_empty_decision(formatter: &mut fmt::Formatter<'_>, decision_id: &DecisionId) -> fmt::Result {
    write!(
        formatter,
        "decision must declare at least one active outcome: {decision_id:?}"
    )
}

fn fmt_raised_owner(
    formatter: &mut fmt::Formatter<'_>,
    action_id: &ActionId,
    event_id: Option<&DomainEventId>,
) -> fmt::Result {
    match event_id {
        None => write!(
            formatter,
            "Action raised-event owner violation: action {action_id:?} is not owned by an Aggregate"
        ),
        Some(event_id) => write!(
            formatter,
            "Action raised-event owner violation: action {action_id:?} declares event {event_id:?} owned by another Aggregate"
        ),
    }
}

fn fmt_action_reference(
    formatter: &mut fmt::Formatter<'_>,
    action_id: &ActionId,
    reference: &DomainModelReference,
    location: &str,
    inventory_key: &str,
) -> fmt::Result {
    write!(
        formatter,
        "Action reference inventory violation: action {action_id:?} references missing {} at descriptor location `{location}`; add it to domain_model! inventory key `{inventory_key}`",
        ReferenceDebug(reference)
    )
}

fn fmt_decision_reference(
    formatter: &mut fmt::Formatter<'_>,
    decision_id: &DecisionId,
    value_object_id: &ValueObjectId,
    location: &str,
) -> fmt::Result {
    write!(
        formatter,
        "Decision reference inventory violation: decision {decision_id:?} references missing {value_object_id:?} at descriptor location `{location}`; add it to domain_model! inventory key `value_objects`"
    )
}

fn fmt_field_reference(
    formatter: &mut fmt::Formatter<'_>,
    reference: &DomainModelReference,
    location: &str,
    inventory_key: &str,
) -> fmt::Result {
    write!(
        formatter,
        "Field reference inventory violation: field references missing {} at descriptor location `{location}`; add it to domain_model! inventory key `{inventory_key}`",
        ReferenceDebug(reference)
    )
}

fn fmt_extension_only_action(
    formatter: &mut fmt::Formatter<'_>,
    lifecycle_id: &EntityLifecycleId,
    action_id: &ActionId,
) -> fmt::Result {
    write!(
        formatter,
        "Entity lifecycle action eligibility violation: lifecycle {lifecycle_id:?} references extension-only action {action_id:?}; action extensions are not eligible for lifecycle transitions"
    )
}

fn fmt_missing_attached_action(
    formatter: &mut fmt::Formatter<'_>,
    lifecycle_id: &EntityLifecycleId,
    action_id: &ActionId,
) -> fmt::Result {
    write!(
        formatter,
        "Entity lifecycle action inventory violation: lifecycle {lifecycle_id:?} references missing attached action {action_id:?}; attach its action contract to the lifecycle owner"
    )
}

fn fmt_lifecycle_owner_mismatch(
    formatter: &mut fmt::Formatter<'_>,
    expected: &EntityId,
    found: &EntityId,
) -> fmt::Result {
    write!(
        formatter,
        "entity lifecycle descriptor owner mismatch: expected {expected:?}, found {found:?}"
    )
}

fn fmt_lifecycle_without_states(
    formatter: &mut fmt::Formatter<'_>,
    lifecycle_id: &EntityLifecycleId,
) -> fmt::Result {
    write!(
        formatter,
        "entity lifecycle descriptor must declare at least one state: {lifecycle_id:?}"
    )
}

fn fmt_duplicate_transition(
    formatter: &mut fmt::Formatter<'_>,
    source: &EntityLifecycleStateId,
    action: &ActionId,
) -> fmt::Result {
    write!(
        formatter,
        "duplicate entity lifecycle transition key: source {source:?}, action {action:?}"
    )
}

fn fmt_state_ownership(
    formatter: &mut fmt::Formatter<'_>,
    location: &str,
    expected: &EntityLifecycleId,
    found: &EntityLifecycleId,
) -> fmt::Result {
    write!(
        formatter,
        "entity lifecycle {location} ownership mismatch: expected {expected:?}, found {found:?}"
    )
}

fn fmt_state_not_declared(
    formatter: &mut fmt::Formatter<'_>,
    location: &str,
    id: &EntityLifecycleStateId,
) -> fmt::Result {
    write!(
        formatter,
        "entity lifecycle {location} is not declared: {id:?}"
    )
}

fn fmt_lifecycle_action_owner_mismatch(
    formatter: &mut fmt::Formatter<'_>,
    expected: &ActionOwnerId,
    found: &ActionOwnerId,
) -> fmt::Result {
    write!(
        formatter,
        "entity lifecycle transition action owner mismatch: expected {expected:?}, found {found:?}"
    )
}

fn fmt_invalid_lifecycle_id(
    formatter: &mut fmt::Formatter<'_>,
    kind: &str,
    local: &str,
) -> fmt::Result {
    write!(
        formatter,
        "entity lifecycle {kind}local id must be nonempty lowercase kebab-case using ASCII letters and digits: {local:?}"
    )
}

fn fmt_empty_lifecycle_label(
    formatter: &mut fmt::Formatter<'_>,
    kind: &str,
    label: &str,
) -> fmt::Result {
    write!(
        formatter,
        "entity lifecycle {kind}label must not be empty: {label:?}"
    )
}

fn fmt_semantic_scalar_mismatch(
    formatter: &mut fmt::Formatter<'_>,
    canonical: ScalarType,
    semantic: ScalarType,
) -> fmt::Result {
    write!(
        formatter,
        "assertion `left == right` failed: DomainIdentity semantic scalar representation must match its canonical scalar descriptor\n  left: {canonical:#?}\n right: {semantic:#?}"
    )
}

struct ReferenceDebug<'a>(&'a DomainModelReference);

impl fmt::Display for ReferenceDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            DomainModelReference::DomainIdentity(id) => write!(formatter, "{id:?}"),
            DomainModelReference::DomainEvent(id) => write!(formatter, "{id:?}"),
            DomainModelReference::DomainError(id) => write!(formatter, "{id:?}"),
            DomainModelReference::ValueObject(id) => write!(formatter, "{id:?}"),
            DomainModelReference::Entity(id) => write!(formatter, "{id:?}"),
            DomainModelReference::Aggregate(id) => write!(formatter, "{id:?}"),
        }
    }
}
