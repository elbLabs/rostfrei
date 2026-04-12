use crate::{ActionId, ActionOwnerId, EntityId, EntityLifecycleDescriptor, EntityLifecycleStateId};

use super::lifecycle_lexical_validation;

pub(super) fn validate(expected_owner: EntityId, descriptor: EntityLifecycleDescriptor) {
    validate_owner(expected_owner, descriptor);
    lifecycle_lexical_validation::validate_lifecycle_id(descriptor);
    lifecycle_lexical_validation::validate_lifecycle_label(descriptor);
    validate_states(descriptor);
    validate_initial(descriptor);
    validate_transitions(expected_owner, descriptor);
}

fn validate_owner(expected_owner: EntityId, descriptor: EntityLifecycleDescriptor) {
    if descriptor.id.owner != expected_owner {
        panic!(
            "entity lifecycle descriptor owner mismatch: expected {expected_owner:?}, found {:?}",
            descriptor.id.owner
        );
    }
}

fn validate_states(descriptor: EntityLifecycleDescriptor) {
    if descriptor.states.is_empty() {
        panic!(
            "entity lifecycle descriptor must declare at least one state: {:?}",
            descriptor.id
        );
    }

    for (index, state) in descriptor.states.iter().enumerate() {
        validate_state_lifecycle("state", state.id, descriptor);
        lifecycle_lexical_validation::validate_state_id(*state);
        lifecycle_lexical_validation::validate_state_label(*state);
        if descriptor.states[..index]
            .iter()
            .any(|preceding| preceding.id == state.id)
        {
            panic!("duplicate EntityLifecycleStateId: {:?}", state.id);
        }
    }
}

fn validate_initial(descriptor: EntityLifecycleDescriptor) {
    validate_state_lifecycle("initial state", descriptor.initial, descriptor);
    validate_declared_state("initial state", descriptor.initial, descriptor);
}

fn validate_transitions(expected_owner: EntityId, descriptor: EntityLifecycleDescriptor) {
    let mut keys = Vec::new();
    for transition in descriptor.transitions {
        validate_state_lifecycle("transition source", transition.source, descriptor);
        validate_declared_state("transition source", transition.source, descriptor);
        validate_state_lifecycle("transition target", transition.target, descriptor);
        validate_declared_state("transition target", transition.target, descriptor);
        validate_action_owner(expected_owner, transition.action);

        let key = (transition.source, transition.action);
        if keys.contains(&key) {
            panic!(
                "duplicate entity lifecycle transition key: source {:?}, action {:?}",
                transition.source, transition.action
            );
        }
        keys.push(key);
    }
}

fn validate_state_lifecycle(
    location: &str,
    state_id: EntityLifecycleStateId,
    descriptor: EntityLifecycleDescriptor,
) {
    if state_id.lifecycle != descriptor.id {
        panic!(
            "entity lifecycle {location} ownership mismatch: expected {:?}, found {:?}",
            descriptor.id, state_id.lifecycle
        );
    }
}

fn validate_declared_state(
    location: &str,
    state_id: EntityLifecycleStateId,
    descriptor: EntityLifecycleDescriptor,
) {
    if !descriptor.states.iter().any(|state| state.id == state_id) {
        panic!("entity lifecycle {location} is not declared: {state_id:?}");
    }
}

fn validate_action_owner(expected_owner: EntityId, action_id: ActionId) {
    let expected = ActionOwnerId::Entity(expected_owner);
    if action_id.owner != expected {
        panic!(
            "entity lifecycle transition action owner mismatch: expected {expected:?}, found {:?}",
            action_id.owner
        );
    }
}
