use crate::{ActionDescriptor, DomainErrorId};

use super::error::DomainModelError;

pub(super) struct ActionErrorInventory {
    domain_errors: Vec<DomainErrorId>,
}

impl ActionErrorInventory {
    pub(super) const fn new(domain_errors: Vec<DomainErrorId>) -> Self {
        Self { domain_errors }
    }
}

pub(super) fn validate<'a>(
    descriptors: impl IntoIterator<Item = &'a ActionDescriptor>,
    inventory: &ActionErrorInventory,
) -> Result<(), DomainModelError> {
    for descriptor in descriptors {
        if let Some(error_id) = descriptor.error
            && !inventory.domain_errors.contains(&error_id)
        {
            return Err(DomainModelError::ActionErrorInventoryViolation {
                action_id: Box::new(descriptor.id),
                error_id: Box::new(error_id),
            });
        }
    }
    Ok(())
}
