use super::EntityLifecycleDescriptor;
use crate::EntityActionOwnerType;

pub trait EntityLifecycleType: 'static {
    type Owner: EntityActionOwnerType;

    const DESCRIPTOR: EntityLifecycleDescriptor;
}
