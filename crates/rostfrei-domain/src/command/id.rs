use super::CommandOwnerId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CommandId {
    pub owner: CommandOwnerId,
    pub local: &'static str,
}
