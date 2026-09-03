use super::EntityLifecycleDescriptor;

pub trait EntityLifecycleType: 'static {
    const DESCRIPTOR: EntityLifecycleDescriptor;
}
