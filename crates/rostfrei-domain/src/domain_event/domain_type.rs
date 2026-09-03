use super::DomainEventDescriptor;
use crate::{AggregateType, DomainEventId, FieldDescriptor};

pub trait DomainEvent: 'static {
    const LOCAL_ID: &'static str;
    const LABEL: &'static str;
    const FIELDS: &'static [FieldDescriptor];
    const SCHEMA_VERSION: u32 = 1;
}

pub trait DomainEventType<A: AggregateType>: DomainEvent {
    const DESCRIPTOR: DomainEventDescriptor = DomainEventDescriptor {
        id: DomainEventId {
            aggregate: A::DESCRIPTOR.id,
            local: Self::LOCAL_ID,
        },
        label: Self::LABEL,
        schema_version: Self::SCHEMA_VERSION,
        fields: Self::FIELDS,
    };
}
