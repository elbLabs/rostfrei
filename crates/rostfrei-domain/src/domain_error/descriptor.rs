use super::DomainErrorId;
use crate::FieldDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DomainErrorDescriptor {
    pub id: DomainErrorId,
    pub label: &'static str,
    pub code: &'static str,
    pub message: &'static str,
    pub fields: &'static [FieldDescriptor],
}
