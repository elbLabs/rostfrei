use super::BoundedContextDescriptor;

pub trait BoundedContextType: 'static {
    const DESCRIPTOR: BoundedContextDescriptor;
}
