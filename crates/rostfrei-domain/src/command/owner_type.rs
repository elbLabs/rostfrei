use super::CommandOwnerId;
use crate::PublicActionOwnerType;

pub trait CommandOwnerType: PublicActionOwnerType {
    const COMMAND_OWNER_ID: CommandOwnerId;
    const COMMAND_NAMESPACE: &'static str;
}
