use crate::{ActionId, ActionOwnerId, EntityId, EntityLifecycleDescriptor, EntityLifecycleStateId};

use super::{error::DomainModelError, lifecycle_lexical_validation};

pub(super) fn validate(
    expected_owner: EntityId,
    descriptor: EntityLifecycleDescriptor,
) -> Result<(), DomainModelError> {
    validate_owner(expected_owner, descriptor)?;
    lifecycle_lexical_validation::validate_lifecycle_id(descriptor)?;
    lifecycle_lexical_validation::validate_lifecycle_label(descriptor)?;
    validate_states(descriptor)?;
    validate_initial(descriptor)?;
    validate_transitions(expected_owner, descriptor)
}

fn validate_owner(
    expected_owner: EntityId,
    descriptor: EntityLifecycleDescriptor,
) -> Result<(), DomainModelError> {
    if descriptor.id.owner != expected_owner {
        return Err(DomainModelError::LifecycleDescriptorOwnerMismatch {
            expected: Box::new(expected_owner),
            found: Box::new(descriptor.id.owner),
        });
    }
    Ok(())
}

fn validate_states(descriptor: EntityLifecycleDescriptor) -> Result<(), DomainModelError> {
    if descriptor.states.is_empty() {
        return Err(DomainModelError::LifecycleWithoutStates {
            lifecycle_id: Box::new(descriptor.id),
        });
    }

    for (index, state) in descriptor.states.iter().enumerate() {
        validate_state_lifecycle("state", state.id, descriptor)?;
        lifecycle_lexical_validation::validate_state_id(*state)?;
        lifecycle_lexical_validation::validate_state_label(*state)?;
        if descriptor
            .states
            .iter()
            .take(index)
            .any(|preceding| preceding.id == state.id)
        {
            return Err(DomainModelError::DuplicateEntityLifecycleStateId {
                id: Box::new(state.id),
            });
        }
    }
    Ok(())
}

fn validate_initial(descriptor: EntityLifecycleDescriptor) -> Result<(), DomainModelError> {
    validate_state_lifecycle("initial state", descriptor.initial, descriptor)?;
    validate_declared_state("initial state", descriptor.initial, descriptor)
}

fn validate_transitions(
    expected_owner: EntityId,
    descriptor: EntityLifecycleDescriptor,
) -> Result<(), DomainModelError> {
    let mut keys = Vec::new();
    for transition in descriptor.transitions {
        validate_state_lifecycle("transition source", transition.source, descriptor)?;
        validate_declared_state("transition source", transition.source, descriptor)?;
        validate_state_lifecycle("transition target", transition.target, descriptor)?;
        validate_declared_state("transition target", transition.target, descriptor)?;
        validate_action_owner(expected_owner, transition.action)?;

        let key = (transition.source, transition.action);
        if keys.contains(&key) {
            return Err(DomainModelError::DuplicateLifecycleTransitionKey {
                source: Box::new(transition.source),
                action: Box::new(transition.action),
            });
        }
        keys.push(key);
    }
    Ok(())
}

fn validate_state_lifecycle(
    location: &'static str,
    state_id: EntityLifecycleStateId,
    descriptor: EntityLifecycleDescriptor,
) -> Result<(), DomainModelError> {
    if state_id.lifecycle != descriptor.id {
        return Err(DomainModelError::LifecycleStateOwnershipMismatch {
            location,
            expected: Box::new(descriptor.id),
            found: Box::new(state_id.lifecycle),
        });
    }
    Ok(())
}

fn validate_declared_state(
    location: &'static str,
    state_id: EntityLifecycleStateId,
    descriptor: EntityLifecycleDescriptor,
) -> Result<(), DomainModelError> {
    if !descriptor.states.iter().any(|state| state.id == state_id) {
        return Err(DomainModelError::LifecycleStateNotDeclared {
            location,
            id: Box::new(state_id),
        });
    }
    Ok(())
}

fn validate_action_owner(
    expected_owner: EntityId,
    action_id: ActionId,
) -> Result<(), DomainModelError> {
    let expected = ActionOwnerId::Entity(expected_owner);
    if action_id.owner != expected {
        return Err(DomainModelError::LifecycleTransitionActionOwnerMismatch {
            expected: Box::new(expected),
            found: Box::new(action_id.owner),
        });
    }
    Ok(())
}
