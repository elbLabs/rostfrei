use std::{error::Error, fmt};

use crate::{AggregateId, DomainErrorId, DomainEventId, DomainIdentityId, EntityId};

/// A domain-model descriptor reference involved in an inventory validation failure.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainModelReference {
    Entity(Box<EntityId>),
    Aggregate(Box<AggregateId>),
}

/// An error encountered while constructing or validating a domain model.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainModelError {
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
    DuplicateDomainErrorId {
        id: Box<DomainErrorId>,
    },
}

impl fmt::Display for DomainModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldReferenceInventoryViolation {
                reference,
                location,
                inventory_key,
            } => fmt_field_reference(formatter, reference, location, inventory_key),
            Self::DuplicateDomainIdentityId { id } => {
                fmt_duplicate(formatter, "DomainIdentityId", id)
            }
            Self::DuplicateDomainEventId { id } => fmt_duplicate(formatter, "DomainEventId", id),
            Self::DuplicateDomainErrorId { id } => fmt_duplicate(formatter, "DomainErrorId", id),
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
            DomainModelReference::Entity(id) => write!(formatter, "{id:?}"),
            DomainModelReference::Aggregate(id) => write!(formatter, "{id:?}"),
        }
    }
}
