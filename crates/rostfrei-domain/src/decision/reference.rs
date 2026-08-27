use super::{DecisionId, DecisionInputType, DecisionOutputType, DecisionOwnerType};
use std::{
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

pub struct DecisionReference<
    Owner: DecisionOwnerType,
    Input: DecisionInputType,
    Output: DecisionOutputType,
> {
    local_id: &'static str,
    owner: PhantomData<fn() -> Owner>,
    input: PhantomData<fn() -> Input>,
    output: PhantomData<fn() -> Output>,
}

impl<Owner, Input, Output> DecisionReference<Owner, Input, Output>
where
    Owner: DecisionOwnerType,
    Input: DecisionInputType,
    Output: DecisionOutputType,
{
    #[doc(hidden)]
    pub const fn __from_local(local_id: &'static str) -> Self {
        Self {
            local_id,
            owner: PhantomData,
            input: PhantomData,
            output: PhantomData,
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

impl<Owner, Input, Output> Copy for DecisionReference<Owner, Input, Output>
where
    Owner: DecisionOwnerType,
    Input: DecisionInputType,
    Output: DecisionOutputType,
{
}

impl<Owner, Input, Output> Clone for DecisionReference<Owner, Input, Output>
where
    Owner: DecisionOwnerType,
    Input: DecisionInputType,
    Output: DecisionOutputType,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<Owner, Input, Output> fmt::Debug for DecisionReference<Owner, Input, Output>
where
    Owner: DecisionOwnerType,
    Input: DecisionInputType,
    Output: DecisionOutputType,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecisionReference")
            .field("id", &self.id())
            .finish()
    }
}

impl<Owner, Input, Output> Eq for DecisionReference<Owner, Input, Output>
where
    Owner: DecisionOwnerType,
    Input: DecisionInputType,
    Output: DecisionOutputType,
{
}

impl<Owner, Input, Output> PartialEq for DecisionReference<Owner, Input, Output>
where
    Owner: DecisionOwnerType,
    Input: DecisionInputType,
    Output: DecisionOutputType,
{
    fn eq(&self, other: &Self) -> bool {
        self.local_id == other.local_id
    }
}

impl<Owner, Input, Output> Hash for DecisionReference<Owner, Input, Output>
where
    Owner: DecisionOwnerType,
    Input: DecisionInputType,
    Output: DecisionOutputType,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}
