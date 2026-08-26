use super::{ActionDescriptor, ActionOwnerType};

pub trait ActionGroupType: 'static {
    type Owner: ActionOwnerType;
    const ACTIONS: &'static [ActionDescriptor];
}
