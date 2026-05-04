use std::{error::Error, fmt};

use super::{EntityLifecycleStateId, EntityLifecycleTransitionId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidStateTransition {
    pub state: EntityLifecycleStateId,
    pub transition: EntityLifecycleTransitionId,
}

impl InvalidStateTransition {
    pub const fn new(
        state: EntityLifecycleStateId,
        transition: EntityLifecycleTransitionId,
    ) -> Self {
        Self { state, transition }
    }
}

impl fmt::Display for InvalidStateTransition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "transition `{}` is not valid from state `{}` in lifecycle `{}`",
            self.transition.local, self.state.local, self.state.lifecycle.0,
        )
    }
}

impl Error for InvalidStateTransition {}
