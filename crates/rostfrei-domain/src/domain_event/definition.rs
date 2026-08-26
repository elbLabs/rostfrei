use crate::FieldDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomainEventDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub schema_version: u32,
    pub fields: &'static [FieldDescriptor],
}

pub trait DomainEventDefinitionType: 'static {
    const DEFINITION: DomainEventDefinition;
}
