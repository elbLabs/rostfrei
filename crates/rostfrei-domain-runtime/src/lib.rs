use domain::{AggregateDefinition, AggregateEventSet, AggregateType};
use rostfrei_core::{Aggregate, Event};

/// Applies one concrete domain event to an aggregate root.
pub trait Apply<Event> {
    fn apply(&mut self, event: &Event);
}

/// Constructs an aggregate root for a stream before its history is replayed.
pub trait Initialize<A: AggregateType>: Sized {
    fn initialize(stream_id: &rostfrei_core::StreamId) -> Self;
}

/// Applies a closed aggregate event set to its aggregate root.
pub trait AggregateEventRuntime<A>: AggregateEventSet<A> + Event
where
    A: AggregateDefinition<Event = Self>,
{
    fn apply(root: &mut <A as AggregateDefinition>::Root, event: &Self);
}

/// Marks a compiled aggregate whose descriptor and executable definition agree.
pub trait AggregateRuntime:
    AggregateDefinition
    + Aggregate<
        State = <Self as AggregateDefinition>::Root,
        Event = <Self as AggregateDefinition>::Event,
    >
{
}

#[doc(hidden)]
pub mod __private {
    pub use rostfrei_core as core;
    pub use rostfrei_core::{Aggregate, AggregateInstance};
    pub use rostfrei_registry::{CommandDefinition, CommandDescriptor};
    pub use std::any::type_name;

    pub use crate::{AggregateEventRuntime, AggregateRuntime, Apply, Initialize};

    pub const fn assert_unique_event_ids(ids: &[&str]) {
        let Some((left, remaining)) = ids.split_first() else {
            return;
        };
        let mut right = remaining;
        while let Some((candidate, rest)) = right.split_first() {
            assert!(
                !strings_equal(left, candidate),
                "duplicate domain event ID in aggregate"
            );
            right = rest;
        }
        assert_unique_event_ids(remaining);
    }

    const fn strings_equal(left: &str, right: &str) -> bool {
        let mut left = left.as_bytes();
        let mut right = right.as_bytes();
        loop {
            match (left.split_first(), right.split_first()) {
                (Some((left_byte, left_rest)), Some((right_byte, right_rest))) => {
                    if *left_byte != *right_byte {
                        return false;
                    }
                    left = left_rest;
                    right = right_rest;
                }
                (None, None) => return true,
                _ => return false,
            }
        }
    }
}
