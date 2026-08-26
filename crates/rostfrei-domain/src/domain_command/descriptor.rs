use super::DomainCommandId;
use crate::FieldDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomainCommandDescriptor {
    pub id: DomainCommandId,
    pub label: &'static str,
    pub fields: &'static [FieldDescriptor],
}
