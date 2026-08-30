use super::{CommandDescriptor, CommandOwnerType};

pub trait CommandType: 'static {
    type Owner: CommandOwnerType;
    type Rejection: 'static;

    const LOCAL_ID: &'static str;
    const SCHEMA_VERSION: u32;
    const DESCRIPTOR: CommandDescriptor;
}
