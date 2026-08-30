use super::CommandId;
use crate::FieldDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandDescriptor {
    pub id: CommandId,
    pub label: &'static str,
    pub fields: &'static [FieldDescriptor],
}
