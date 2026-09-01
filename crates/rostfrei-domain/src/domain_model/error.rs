use std::{error::Error, fmt};

use crate::{
    ActionId, ActionOwnerId, AggregateId, CommandId, DecisionId, DecisionOutcomeId,
    DecisionOwnerId, DomainErrorId, DomainEventId, DomainIdentityId, EntityId, QueryId, ScalarType,
    ValueObjectId,
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
    DuplicateCommandId {
        id: Box<CommandId>,
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
            Self::DomainIdentitySemanticScalarRepresentationMismatch {
                canonical,
                semantic,
            } => fmt_semantic_scalar_mismatch(formatter, *canonical, *semantic),
            Self::DuplicateDomainIdentityId { id } => {
                fmt_duplicate(formatter, "DomainIdentityId", id)
            }
            Self::DuplicateDomainEventId { id } => fmt_duplicate(formatter, "DomainEventId", id),
            Self::DuplicateCommandId { id } => fmt_duplicate(formatter, "CommandId", id),
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
