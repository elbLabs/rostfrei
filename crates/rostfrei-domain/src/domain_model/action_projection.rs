use serde_json::{Value, json};

use crate::{ActionDescriptor, ActionId, ActionOwnerId};

use super::{
    action_error_validation::{self, ActionErrorInventory},
    error::DomainModelError,
    id_projection::{action as action_id, domain_error as domain_error_id},
};

pub(super) struct ActionProjection {
    registered_owners: Vec<ActionOwnerId>,
    actions: Vec<(ActionDescriptor, Value)>,
    extensions: Vec<(ActionDescriptor, Value)>,
}

impl ActionProjection {
    pub(super) const fn new() -> Self {
        Self {
            registered_owners: Vec::new(),
            actions: Vec::new(),
            extensions: Vec::new(),
        }
    }

    pub(super) fn register_owner(&mut self, owner: ActionOwnerId) {
        if !self.registered_owners.contains(&owner) {
            self.registered_owners.push(owner);
        }
    }

    pub(super) fn add_group(
        &mut self,
        expected_owner: ActionOwnerId,
        descriptors: &'static [ActionDescriptor],
    ) -> Result<(), DomainModelError> {
        self.validate_group(expected_owner, descriptors)?;
        Self::append(&mut self.actions, descriptors);
        Ok(())
    }

    pub(super) fn add_extension(
        &mut self,
        expected_owner: ActionOwnerId,
        descriptors: &'static [ActionDescriptor],
    ) -> Result<(), DomainModelError> {
        self.validate_extension(expected_owner, descriptors)?;
        Self::append(&mut self.extensions, descriptors);
        Ok(())
    }

    pub(super) fn validate_errors(
        &self,
        inventory: &ActionErrorInventory,
    ) -> Result<(), DomainModelError> {
        action_error_validation::validate(
            self.actions
                .iter()
                .chain(&self.extensions)
                .map(|(descriptor, _)| descriptor),
            inventory,
        )
    }

    pub(super) fn into_values(self) -> Vec<Value> {
        self.actions
            .into_iter()
            .chain(self.extensions)
            .map(|(_, value)| value)
            .collect()
    }

    fn validate_extension(
        &self,
        expected_owner: ActionOwnerId,
        descriptors: &'static [ActionDescriptor],
    ) -> Result<(), DomainModelError> {
        if !self.registered_owners.contains(&expected_owner) {
            return Err(DomainModelError::UnregisteredActionExtensionOwner {
                owner: Box::new(expected_owner),
            });
        }
        if descriptors.is_empty() {
            return Err(DomainModelError::EmptyActionExtension);
        }
        self.validate_group(expected_owner, descriptors)
    }

    fn validate_group(
        &self,
        expected_owner: ActionOwnerId,
        descriptors: &'static [ActionDescriptor],
    ) -> Result<(), DomainModelError> {
        for (index, descriptor) in descriptors.iter().enumerate() {
            Self::validate_owner(expected_owner, descriptor)?;
            self.validate_id(descriptor.id, descriptors.iter().take(index))?;
        }
        Ok(())
    }

    fn validate_owner(
        expected_owner: ActionOwnerId,
        descriptor: &ActionDescriptor,
    ) -> Result<(), DomainModelError> {
        if descriptor.id.owner != expected_owner {
            return Err(DomainModelError::ActionDescriptorOwnerMismatch {
                id: Box::new(descriptor.id),
            });
        }
        Ok(())
    }

    fn validate_id<'a>(
        &self,
        id: ActionId,
        preceding: impl Iterator<Item = &'a ActionDescriptor>,
    ) -> Result<(), DomainModelError> {
        if self.has_id(id) || preceding.into_iter().any(|descriptor| descriptor.id == id) {
            return Err(DomainModelError::DuplicateActionId { id: Box::new(id) });
        }
        Ok(())
    }

    fn has_id(&self, id: ActionId) -> bool {
        self.actions
            .iter()
            .chain(&self.extensions)
            .any(|(descriptor, _)| descriptor.id == id)
    }

    fn append(target: &mut Vec<(ActionDescriptor, Value)>, descriptors: &[ActionDescriptor]) {
        target.extend(
            descriptors
                .iter()
                .map(|descriptor| (*descriptor, action(descriptor))),
        );
    }
}

fn action(descriptor: &ActionDescriptor) -> Value {
    json!({
        "id": action_id(descriptor.id),
        "label": descriptor.label,
        "error": descriptor.error.map(domain_error_id),
    })
}
