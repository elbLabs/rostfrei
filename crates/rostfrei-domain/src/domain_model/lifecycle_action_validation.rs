use crate::{ActionId, EntityLifecycleDescriptor};

pub(super) struct LifecycleActionInventory {
    attached: Vec<ActionId>,
    extensions: Vec<ActionId>,
}

impl LifecycleActionInventory {
    pub(super) fn new(attached: Vec<ActionId>, extensions: Vec<ActionId>) -> Self {
        Self {
            attached,
            extensions,
        }
    }
}

pub(super) fn validate(
    descriptors: impl IntoIterator<Item = EntityLifecycleDescriptor>,
    inventory: &LifecycleActionInventory,
) {
    for descriptor in descriptors {
        for transition in descriptor.transitions {
            validate_action(descriptor, transition.action, inventory);
        }
    }
}

fn validate_action(
    descriptor: EntityLifecycleDescriptor,
    action_id: ActionId,
    inventory: &LifecycleActionInventory,
) {
    if inventory.attached.contains(&action_id) {
        return;
    }
    if inventory.extensions.contains(&action_id) {
        panic!(
            "Entity lifecycle action eligibility violation: lifecycle {:?} references extension-only action {action_id:?}; action extensions are not eligible for lifecycle transitions",
            descriptor.id
        );
    }
    panic!(
        "Entity lifecycle action inventory violation: lifecycle {:?} references missing attached action {action_id:?}; attach its action contract to the lifecycle owner",
        descriptor.id
    );
}
