use super::{AttachedDecisionGroup, DecisionGroupType, DecisionId, DecisionOwnerType};
use std::{
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
};

pub struct DecisionReference<Group: DecisionGroupType> {
    local_id: &'static str,
    group: PhantomData<fn() -> Group>,
}

impl<Group: DecisionGroupType> DecisionReference<Group> {
    #[doc(hidden)]
    pub const fn __from_local(local_id: &'static str) -> Self {
        Self {
            local_id,
            group: PhantomData,
        }
    }

    pub const fn id(&self) -> DecisionId {
        DecisionId {
            owner: <Group::Owner as DecisionOwnerType>::DECISION_OWNER_ID,
            local: self.local_id,
        }
    }

    pub const fn local_id(&self) -> &'static str {
        self.local_id
    }

    #[doc(hidden)]
    pub const fn __attached_id(&self) -> DecisionId
    where
        Group::Owner: AttachedDecisionGroup<Group>,
    {
        self.id()
    }
}

impl<Group: DecisionGroupType> Copy for DecisionReference<Group> {}

impl<Group: DecisionGroupType> Clone for DecisionReference<Group> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Group: DecisionGroupType> fmt::Debug for DecisionReference<Group> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecisionReference")
            .field("id", &self.id())
            .finish()
    }
}

impl<Group: DecisionGroupType> Eq for DecisionReference<Group> {}

impl<Group: DecisionGroupType> PartialEq for DecisionReference<Group> {
    fn eq(&self, other: &Self) -> bool {
        self.local_id == other.local_id
    }
}

impl<Group: DecisionGroupType> Hash for DecisionReference<Group> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use std::{hash::Hash, mem::size_of};

    use crate::{AggregateId, BoundedContextId, DecisionDescriptor, DecisionOwnerId};

    use super::*;

    const OWNER_ID: DecisionOwnerId = DecisionOwnerId::Aggregate(AggregateId {
        context: BoundedContextId("reference-test"),
        local: "owner",
    });

    struct Owner;

    impl DecisionOwnerType for Owner {
        const DECISION_OWNER_ID: DecisionOwnerId = OWNER_ID;
    }

    struct Group;

    impl DecisionGroupType for Group {
        type Owner = Owner;

        const DECISIONS: &'static [DecisionDescriptor] = &[];
    }

    impl AttachedDecisionGroup<Group> for Owner {}

    #[test]
    fn derives_owner_and_preserves_bound_free_reference_behavior() {
        fn assert_traits<T: Copy + Clone + Eq + Hash>() {}

        assert_traits::<DecisionReference<Group>>();

        let reference = DecisionReference::<Group>::__from_local("approve");
        assert_eq!(reference.local_id(), "approve");
        assert_eq!(reference.id().owner, OWNER_ID);
        assert_eq!(reference.__attached_id(), reference.id());
        assert_eq!(size_of::<DecisionReference<Group>>(), size_of::<&str>());
    }
}
