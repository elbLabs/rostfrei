use super::ActionOwnerId;

pub trait ActionOwnerType: 'static {
    const ACTION_OWNER_ID: ActionOwnerId;
}
