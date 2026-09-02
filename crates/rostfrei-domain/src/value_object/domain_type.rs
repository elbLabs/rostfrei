use super::ValueObjectDescriptor;

pub trait ValueObject: 'static {
    const DESCRIPTOR: ValueObjectDescriptor;
}
