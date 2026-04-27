use super::InvariantId;
use std::fmt;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct InvariantReference {
    local_id: &'static str,
}

impl InvariantReference {
    #[doc(hidden)]
    pub const fn __from_local(local_id: &'static str) -> Self {
        Self { local_id }
    }

    pub const fn id(&self) -> InvariantId {
        InvariantId(self.local_id)
    }

    pub const fn local_id(&self) -> &'static str {
        self.local_id
    }
}

impl fmt::Debug for InvariantReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InvariantReference")
            .field("id", &self.id())
            .finish()
    }
}
