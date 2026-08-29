use crate::{
    ActionDescriptor, ActionId, ActionInputDescriptor, ActionOutputDescriptor, ActionOwnerId,
    DomainErrorId, DomainEventId, DomainIdentityId, ValueObjectId,
};

use super::error::{DomainModelError, DomainModelReference};

pub(super) struct ActionReferenceInventory {
    domain_identities: Vec<DomainIdentityId>,
    domain_events: Vec<DomainEventId>,
    domain_errors: Vec<DomainErrorId>,
    value_objects: Vec<ValueObjectId>,
}

impl ActionReferenceInventory {
    pub(super) const fn new(
        domain_identities: Vec<DomainIdentityId>,
        domain_events: Vec<DomainEventId>,
        domain_errors: Vec<DomainErrorId>,
        value_objects: Vec<ValueObjectId>,
    ) -> Self {
        Self {
            domain_identities,
            domain_events,
            domain_errors,
            value_objects,
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
    if let Some(input) = descriptor.input {
        validate_input_reference(descriptor.id, input, inventory)?;
    }
    if let Some(output) = descriptor.output {
        validate_output_references(descriptor.id, output, "output", inventory)?;
    }
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

fn validate_input_reference(
    action_id: ActionId,
    input: ActionInputDescriptor,
    inventory: &ActionReferenceInventory,
) -> Result<(), DomainModelError> {
    match input {
        ActionInputDescriptor::Scalar(_) => Ok(()),
        ActionInputDescriptor::ValueObject(id) => {
            if inventory.value_objects.contains(&id) {
                Ok(())
            } else {
                Err(missing_reference(
                    action_id,
                    DomainModelReference::ValueObject(Box::new(id)),
                    "input",
                    "value_objects",
                ))
            }
        }
        ActionInputDescriptor::DomainIdentity(id) => {
            if inventory.domain_identities.contains(&id) {
                Ok(())
            } else {
                Err(missing_reference(
                    action_id,
                    DomainModelReference::DomainIdentity(Box::new(id)),
                    "input",
                    "identities",
                ))
            }
        }
    }
}

fn validate_output_references(
    action_id: ActionId,
    output: ActionOutputDescriptor,
    location: &str,
    inventory: &ActionReferenceInventory,
) -> Result<(), DomainModelError> {
    match output {
        ActionOutputDescriptor::Scalar(_) => Ok(()),
        ActionOutputDescriptor::ValueObject(id) => {
            if inventory.value_objects.contains(&id) {
                Ok(())
            } else {
                Err(missing_reference(
                    action_id,
                    DomainModelReference::ValueObject(Box::new(id)),
                    location,
                    "value_objects",
                ))
            }
        }
        ActionOutputDescriptor::DomainEvent(id) => {
            if inventory.domain_events.contains(&id) {
                Ok(())
            } else {
                Err(missing_reference(
                    action_id,
                    DomainModelReference::DomainEvent(Box::new(id)),
                    location,
                    "events",
                ))
            }
        }
        ActionOutputDescriptor::Optional(value) => {
            let location = format!("{location}.optional.value");
            validate_output_references(action_id, *value, &location, inventory)
        }
        ActionOutputDescriptor::List(element) => {
            let location = format!("{location}.list.element");
            validate_output_references(action_id, *element, &location, inventory)
        }
    }
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
