use super::AggregateDescriptor;

pub trait AggregateType: 'static + Sized {
    const DESCRIPTOR: AggregateDescriptor;
}
