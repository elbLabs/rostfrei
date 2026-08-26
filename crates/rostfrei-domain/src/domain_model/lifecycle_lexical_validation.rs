use crate::{EntityLifecycleDescriptor, EntityLifecycleStateDescriptor};

pub(super) fn validate_lifecycle_id(descriptor: EntityLifecycleDescriptor) {
    if !valid_id(descriptor.id.local) {
        panic!(
            "entity lifecycle local id must be nonempty lowercase kebab-case using ASCII letters and digits: {:?}",
            descriptor.id.local
        );
    }
}

pub(super) fn validate_lifecycle_label(descriptor: EntityLifecycleDescriptor) {
    if descriptor.label.trim().is_empty() {
        panic!(
            "entity lifecycle label must not be empty: {:?}",
            descriptor.label
        );
    }
}

pub(super) fn validate_state_id(descriptor: EntityLifecycleStateDescriptor) {
    if !valid_id(descriptor.id.local) {
        panic!(
            "entity lifecycle state local id must be nonempty lowercase kebab-case using ASCII letters and digits: {:?}",
            descriptor.id.local
        );
    }
}

pub(super) fn validate_state_label(descriptor: EntityLifecycleStateDescriptor) {
    if descriptor.label.trim().is_empty() {
        panic!(
            "entity lifecycle state label must not be empty: {:?}",
            descriptor.label
        );
    }
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
