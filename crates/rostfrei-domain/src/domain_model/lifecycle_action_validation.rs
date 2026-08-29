use crate::{ActionId, EntityLifecycleDescriptor};

use super::error::DomainModelError;

pub(super) struct LifecycleActionInventory {
    attached: Vec<ActionId>,
    extensions: Vec<ActionId>,
}

impl LifecycleActionInventory {
    pub(super) const fn new(attached: Vec<ActionId>, extensions: Vec<ActionId>) -> Self {
        Self {
            attached,
            extensions,
        }
    }
}

pub(super) fn validate(
    descriptors: impl IntoIterator<Item = EntityLifecycleDescriptor>,
    inventory: &LifecycleActionInventory,
) -> Result<(), DomainModelError> {
    for descriptor in descriptors {
        for transition in descriptor.transitions {
            validate_action(descriptor, transition.action, inventory)?;
        }
    }
    Ok(())
}

fn validate_action(
    descriptor: EntityLifecycleDescriptor,
    action_id: ActionId,
    inventory: &LifecycleActionInventory,
) -> Result<(), DomainModelError> {
    if inventory.attached.contains(&action_id) {
        return Ok(());
    }
    if inventory.extensions.contains(&action_id) {
        return Err(DomainModelError::LifecycleExtensionOnlyAction {
            lifecycle_id: Box::new(descriptor.id),
            action_id: Box::new(action_id),
        });
    }
    Err(DomainModelError::LifecycleMissingAttachedAction {
        lifecycle_id: Box::new(descriptor.id),
        action_id: Box::new(action_id),
    })
}
