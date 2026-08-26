use super::{ActionId, ActionOwnerType};
use std::{
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

pub struct ActionReference<Owner: ActionOwnerType> {
    local_id: &'static str,
    owner: PhantomData<fn() -> Owner>,
}

impl<Owner: ActionOwnerType> ActionReference<Owner> {
    #[doc(hidden)]
    pub const fn __from_local(local_id: &'static str) -> Self {
        Self {
            local_id,
            owner: PhantomData,
        }
    }

    pub const fn id(&self) -> ActionId {
        ActionId {
            owner: Owner::ACTION_OWNER_ID,
            local: self.local_id,
        }
    }

    pub const fn local_id(&self) -> &'static str {
        self.local_id
    }
}

impl<Owner: ActionOwnerType> Copy for ActionReference<Owner> {}

impl<Owner: ActionOwnerType> Clone for ActionReference<Owner> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Owner: ActionOwnerType> fmt::Debug for ActionReference<Owner> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActionReference")
            .field("id", &self.id())
            .finish()
    }
}

impl<Owner: ActionOwnerType> Eq for ActionReference<Owner> {}

impl<Owner: ActionOwnerType> PartialEq for ActionReference<Owner> {
    fn eq(&self, other: &Self) -> bool {
        self.local_id == other.local_id
    }
}

impl<Owner: ActionOwnerType> Hash for ActionReference<Owner> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}
