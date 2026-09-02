use std::{error::Error, fmt};

use crate::{
    ActionId, ActionOwnerId, AggregateId, DecisionId, DecisionOutcomeId, DecisionOwnerId,
    DomainErrorId, DomainEventId, DomainIdentityId, EntityId, QueryId, ValueObjectId,
};

/// A domain-model descriptor reference involved in an inventory validation failure.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainModelReference {
    DomainIdentity(Box<DomainIdentityId>),
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
    ActionErrorInventoryViolation {
        action_id: Box<ActionId>,
        error_id: Box<DomainErrorId>,
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
    FieldReferenceInventoryViolation {
        reference: DomainModelReference,
        location: String,
        inventory_key: &'static str,
    },
    DuplicateDomainIdentityId {
        id: Box<DomainIdentityId>,
    },
    DuplicateDomainEventId {
        id: Box<DomainEventId>,
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
            Self::ActionErrorInventoryViolation {
                action_id,
                error_id,
            } => fmt_action_error(formatter, action_id, error_id),
            Self::UnregisteredDecisionOwner { owner } => fmt_unregistered_owner(formatter, owner),
            Self::DecisionDescriptorOwnerMismatch { id } => fmt_decision_owner(formatter, id),
            Self::DuplicateDecisionId { id } => write!(formatter, "duplicate DecisionId: {id:?}"),
            Self::DecisionWithoutOutcomes { decision_id } => {
                fmt_empty_decision(formatter, decision_id)
            }
            Self::DuplicateDecisionOutcomeId { id } => fmt_duplicate_outcome(formatter, id),
            Self::FieldReferenceInventoryViolation {
                reference,
                location,
                inventory_key,
            } => fmt_field_reference(formatter, reference, location, inventory_key),
            Self::DuplicateDomainIdentityId { id } => {
                fmt_duplicate(formatter, "DomainIdentityId", id)
            }
            Self::DuplicateDomainEventId { id } => fmt_duplicate(formatter, "DomainEventId", id),
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

fn fmt_action_error(
    formatter: &mut fmt::Formatter<'_>,
    action_id: &ActionId,
    error_id: &DomainErrorId,
) -> fmt::Result {
    write!(
        formatter,
        "Action error inventory violation: action {action_id:?} references missing {error_id:?}; add it to domain_model! inventory key `errors`"
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

struct ReferenceDebug<'a>(&'a DomainModelReference);

impl fmt::Display for ReferenceDebug<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            DomainModelReference::DomainIdentity(id) => write!(formatter, "{id:?}"),
            DomainModelReference::ValueObject(id) => write!(formatter, "{id:?}"),
            DomainModelReference::Entity(id) => write!(formatter, "{id:?}"),
            DomainModelReference::Aggregate(id) => write!(formatter, "{id:?}"),
        }
    }
}
