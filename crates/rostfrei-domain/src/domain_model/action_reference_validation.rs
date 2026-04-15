use crate::{
    ActionDescriptor, ActionId, ActionInputDescriptor, ActionOutputDescriptor, DomainErrorId,
    DomainEventId, DomainIdentityId, ValueObjectId,
};

pub(super) struct ActionReferenceInventory {
    domain_identities: Vec<DomainIdentityId>,
    domain_events: Vec<DomainEventId>,
    domain_errors: Vec<DomainErrorId>,
    value_objects: Vec<ValueObjectId>,
}

impl ActionReferenceInventory {
    pub(super) fn new(
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

pub(super) fn validate(
    descriptors: impl IntoIterator<Item = ActionDescriptor>,
    inventory: &ActionReferenceInventory,
) {
    for descriptor in descriptors {
        validate_references(descriptor, inventory);
    }
}

fn validate_references(descriptor: ActionDescriptor, inventory: &ActionReferenceInventory) {
    if let Some(input) = descriptor.input {
        validate_input_reference(descriptor.id, input, inventory);
    }
    if let Some(output) = descriptor.output {
        validate_output_references(descriptor.id, output, "output", inventory);
    }
    if let Some(id) = descriptor.error
        && !inventory.domain_errors.contains(&id)
    {
        missing_reference(descriptor.id, id, "error", "errors");
    }
}

fn validate_input_reference(
    action_id: ActionId,
    input: ActionInputDescriptor,
    inventory: &ActionReferenceInventory,
) {
    match input {
        ActionInputDescriptor::Scalar(_) => {}
        ActionInputDescriptor::ValueObject(id) => {
            if !inventory.value_objects.contains(&id) {
                missing_reference(action_id, id, "input", "value_objects");
            }
        }
        ActionInputDescriptor::DomainIdentity(id) => {
            if !inventory.domain_identities.contains(&id) {
                missing_reference(action_id, id, "input", "identities");
            }
        }
    }
}

fn validate_output_references(
    action_id: ActionId,
    output: ActionOutputDescriptor,
    location: &str,
    inventory: &ActionReferenceInventory,
) {
    match output {
        ActionOutputDescriptor::Scalar(_) => {}
        ActionOutputDescriptor::ValueObject(id) => {
            if !inventory.value_objects.contains(&id) {
                missing_reference(action_id, id, location, "value_objects");
            }
        }
        ActionOutputDescriptor::DomainEvent(id) => {
            if !inventory.domain_events.contains(&id) {
                missing_reference(action_id, id, location, "events");
            }
        }
        ActionOutputDescriptor::Optional(value) => {
            let location = format!("{location}.optional.value");
            validate_output_references(action_id, *value, &location, inventory);
        }
        ActionOutputDescriptor::List(element) => {
            let location = format!("{location}.list.element");
            validate_output_references(action_id, *element, &location, inventory);
        }
    }
}

fn missing_reference(
    action_id: ActionId,
    item_id: impl std::fmt::Debug,
    location: &str,
    inventory_key: &str,
) -> ! {
    panic!(
        "Action reference inventory violation: action {action_id:?} references missing {item_id:?} at descriptor location `{location}`; add it to domain_model! inventory key `{inventory_key}`"
    );
}
