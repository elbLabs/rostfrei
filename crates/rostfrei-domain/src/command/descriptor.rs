use crate::{BoundedContextDescriptor, FieldDescriptor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandDescriptor {
    pub bounded_context: BoundedContextDescriptor,
    pub local_id: &'static str,
    pub label: &'static str,
    pub fields: &'static [FieldDescriptor],
    pub schema_version: u32,
}
