use crate::FieldDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandDescriptor {
    pub local_id: &'static str,
    pub label: &'static str,
    pub fields: &'static [FieldDescriptor],
    pub schema_version: u32,
}
