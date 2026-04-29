use crate::{ActionDescriptor, ActionId, ActionOwnerId, DomainErrorId, DomainEventId};

use super::error::{DomainModelError, DomainModelReference};

pub(super) struct ActionReferenceInventory {
    domain_events: Vec<DomainEventId>,
    domain_errors: Vec<DomainErrorId>,
}

impl ActionReferenceInventory {
    pub(super) const fn new(
        domain_events: Vec<DomainEventId>,
        domain_errors: Vec<DomainErrorId>,
    ) -> Self {
        Self {
            domain_events,
            domain_errors,
        }
    }
}

pub(super) fn validate<'a>(
    descriptors: impl IntoIterator<Item = &'a ActionDescriptor>,
    inventory: &ActionReferenceInventory,
) -> Result<(), DomainModelError> {
    for descriptor in descriptors {
        validate_references(descriptor, inventory)?;
    }
    Ok(())
}

fn validate_references(
    descriptor: &ActionDescriptor,
    inventory: &ActionReferenceInventory,
) -> Result<(), DomainModelError> {
    for (index, id) in descriptor.raises.iter().enumerate() {
        let ActionOwnerId::Aggregate(owner) = descriptor.id.owner else {
            return Err(DomainModelError::ActionRaisedEventOwnerNotAggregate {
                action_id: Box::new(descriptor.id),
            });
        };
        if id.aggregate != owner {
            return Err(DomainModelError::ActionRaisedEventOwnerMismatch {
                action_id: Box::new(descriptor.id),
                event_id: Box::new(*id),
            });
        }
        if !inventory.domain_events.contains(id) {
            return Err(missing_reference(
                descriptor.id,
                DomainModelReference::DomainEvent(Box::new(*id)),
                &format!("raises[{index}]"),
                "events",
            ));
        }
    }
    if let Some(id) = descriptor.error
        && !inventory.domain_errors.contains(&id)
    {
        return Err(missing_reference(
            descriptor.id,
            DomainModelReference::DomainError(Box::new(id)),
            "error",
            "errors",
        ));
    }
    Ok(())
}

fn missing_reference(
    action_id: ActionId,
    reference: DomainModelReference,
    location: &str,
    inventory_key: &'static str,
) -> DomainModelError {
    DomainModelError::ActionReferenceInventoryViolation {
        action_id: Box::new(action_id),
        reference,
        location: location.to_owned(),
        inventory_key,
    }
}
