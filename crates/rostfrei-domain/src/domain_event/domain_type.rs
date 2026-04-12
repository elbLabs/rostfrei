use super::DomainEventDescriptor;
use crate::AggregateType;

pub trait DomainEventType: 'static {
    type Owner: AggregateType;

    const LOCAL_ID: &'static str;
    const DESCRIPTOR: DomainEventDescriptor;
}
