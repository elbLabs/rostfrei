use super::DomainEventDescriptor;
use crate::{AggregateType, DomainEventDefinitionType, DomainEventId};

pub trait DomainEventType<A: AggregateType>: DomainEventDefinitionType {
    const LOCAL_ID: &'static str = Self::DEFINITION.id;
    const SCHEMA_VERSION: u32 = Self::DEFINITION.schema_version;
    const DESCRIPTOR: DomainEventDescriptor = DomainEventDescriptor {
        id: DomainEventId {
            aggregate: A::DESCRIPTOR.id,
            local: Self::DEFINITION.id,
        },
        label: Self::DEFINITION.label,
        schema_version: Self::DEFINITION.schema_version,
        fields: Self::DEFINITION.fields,
    };
}
