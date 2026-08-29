use crate::{EntityLifecycleDescriptor, EntityLifecycleStateDescriptor};

use super::error::DomainModelError;

pub(super) fn validate_lifecycle_id(
    descriptor: EntityLifecycleDescriptor,
) -> Result<(), DomainModelError> {
    if !valid_id(descriptor.id.local) {
        return Err(DomainModelError::InvalidLifecycleLocalId {
            local: descriptor.id.local,
        });
    }
    Ok(())
}

pub(super) fn validate_lifecycle_label(
    descriptor: EntityLifecycleDescriptor,
) -> Result<(), DomainModelError> {
    if descriptor.label.trim().is_empty() {
        return Err(DomainModelError::EmptyLifecycleLabel {
            label: descriptor.label,
        });
    }
    Ok(())
}

pub(super) fn validate_state_id(
    descriptor: EntityLifecycleStateDescriptor,
) -> Result<(), DomainModelError> {
    if !valid_id(descriptor.id.local) {
        return Err(DomainModelError::InvalidLifecycleStateLocalId {
            local: descriptor.id.local,
        });
    }
    Ok(())
}

pub(super) fn validate_state_label(
    descriptor: EntityLifecycleStateDescriptor,
) -> Result<(), DomainModelError> {
    if descriptor.label.trim().is_empty() {
        return Err(DomainModelError::EmptyLifecycleStateLabel {
            label: descriptor.label,
        });
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}
