use super::{DecisionId, DecisionOwnerType};
use std::{
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

pub struct DecisionReference<Owner: DecisionOwnerType> {
    local_id: &'static str,
    owner: PhantomData<fn() -> Owner>,
}

impl<Owner: DecisionOwnerType> DecisionReference<Owner> {
    #[doc(hidden)]
    pub const fn __from_local(local_id: &'static str) -> Self {
        Self {
            local_id,
            owner: PhantomData,
        }
    }

    pub const fn id(&self) -> DecisionId {
        DecisionId {
            owner: Owner::DECISION_OWNER_ID,
            local: self.local_id,
        }
    }

    pub const fn local_id(&self) -> &'static str {
        self.local_id
    }
}

impl<Owner: DecisionOwnerType> Copy for DecisionReference<Owner> {}

impl<Owner: DecisionOwnerType> Clone for DecisionReference<Owner> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Owner: DecisionOwnerType> fmt::Debug for DecisionReference<Owner> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecisionReference")
            .field("id", &self.id())
            .finish()
    }
}

impl<Owner: DecisionOwnerType> Eq for DecisionReference<Owner> {}

impl<Owner: DecisionOwnerType> PartialEq for DecisionReference<Owner> {
    fn eq(&self, other: &Self) -> bool {
        self.local_id == other.local_id
    }
}

impl<Owner: DecisionOwnerType> Hash for DecisionReference<Owner> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}
