use super::DomainEventId;
use crate::FieldDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomainEventDescriptor {
    pub id: DomainEventId,
    pub label: &'static str,
    pub schema_version: u32,
    pub fields: &'static [FieldDescriptor],
}
