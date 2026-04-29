use super::CommandDescriptor;
use crate::FieldDescriptor;

pub trait Command: 'static {
    const LOCAL_ID: &'static str;
    const LABEL: &'static str;
    const FIELDS: &'static [FieldDescriptor];
    const SCHEMA_VERSION: u32 = 1;
    const DESCRIPTOR: CommandDescriptor = CommandDescriptor {
        local_id: Self::LOCAL_ID,
        label: Self::LABEL,
        fields: Self::FIELDS,
        schema_version: Self::SCHEMA_VERSION,
    };
}
