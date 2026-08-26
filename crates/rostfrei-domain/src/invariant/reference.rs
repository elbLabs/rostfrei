use super::{InvariantId, InvariantOwnerType};
use std::{
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

pub struct InvariantReference<Owner: InvariantOwnerType> {
    local_id: &'static str,
    owner: PhantomData<fn() -> Owner>,
}

impl<Owner: InvariantOwnerType> InvariantReference<Owner> {
    #[doc(hidden)]
    pub const fn __from_local(local_id: &'static str) -> Self {
        Self {
            local_id,
            owner: PhantomData,
        }
    }

    pub const fn id(&self) -> InvariantId {
        InvariantId {
            owner: Owner::INVARIANT_OWNER_ID,
            local: self.local_id,
        }
    }

    pub const fn local_id(&self) -> &'static str {
        self.local_id
    }
}

impl<Owner: InvariantOwnerType> Copy for InvariantReference<Owner> {}

impl<Owner: InvariantOwnerType> Clone for InvariantReference<Owner> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Owner: InvariantOwnerType> fmt::Debug for InvariantReference<Owner> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InvariantReference")
            .field("id", &self.id())
            .finish()
    }
}

impl<Owner: InvariantOwnerType> Eq for InvariantReference<Owner> {}

impl<Owner: InvariantOwnerType> PartialEq for InvariantReference<Owner> {
    fn eq(&self, other: &Self) -> bool {
        self.local_id == other.local_id
    }
}

impl<Owner: InvariantOwnerType> Hash for InvariantReference<Owner> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}
