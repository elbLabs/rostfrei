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
    pub use domain::{CommandOwnerType, CommandType};
    pub use rostfrei_core as core;
    pub use rostfrei_core::{Aggregate, AggregateInstance};
    pub use rostfrei_registry::{
        CommandDefinition, CommandDescriptor, DomainModule, ModuleDescriptor, QueryDefinition,
    };
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

    pub const fn assert_same_command_namespace(left: &str, right: &str) {
        assert!(
            strings_equal(left, right),
            "domain module commands must belong to the same namespace"
        );
    }

    pub const fn assert_same_module_namespace(left: &str, right: &str) {
        assert!(
            strings_equal(left, right),
            "domain module commands and queries must belong to the same namespace"
        );
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

/// Groups commands and queries into a named registry module.
///
/// Command derives provide their runtime definitions. This macro only
/// supplies an optional module-level grouping for applications that need one.
#[macro_export]
macro_rules! domain_module {
    (
        $(#[$attribute:meta])*
        $visibility:vis struct $module:ident {
            commands: [$first:ty $(, $command:ty)* $(,)?],
            queries: [$first_query:ty $(, $query:ty)* $(,)?] $(,)?
        }
    ) => {
        const _: () = {
            $(
                $crate::__private::assert_same_command_namespace(
                    <<$first as $crate::__private::CommandType>::Owner as
                        $crate::__private::CommandOwnerType>::COMMAND_NAMESPACE,
                    <<$command as $crate::__private::CommandType>::Owner as
                        $crate::__private::CommandOwnerType>::COMMAND_NAMESPACE,
                );
            )*
            $crate::__private::assert_same_module_namespace(
                <<$first as $crate::__private::CommandType>::Owner as
                    $crate::__private::CommandOwnerType>::COMMAND_NAMESPACE,
                <$first_query as $crate::__private::QueryDefinition>::BOUNDED_CONTEXT,
            );
            $(
                $crate::__private::assert_same_module_namespace(
                    <$first_query as $crate::__private::QueryDefinition>::BOUNDED_CONTEXT,
                    <$query as $crate::__private::QueryDefinition>::BOUNDED_CONTEXT,
                );
            )*
        };

        $(#[$attribute])*
        $visibility struct $module;

        impl $crate::__private::DomainModule for $module {
            const MODULE_NAME: &'static str =
                <<$first as $crate::__private::CommandType>::Owner as
                    $crate::__private::CommandOwnerType>::COMMAND_NAMESPACE;

            fn descriptor() -> $crate::__private::ModuleDescriptor {
                $crate::__private::ModuleDescriptor {
                    module_name: Self::MODULE_NAME,
                    commands: ::std::vec![
                        <$first as $crate::__private::CommandDefinition>::descriptor(),
                        $(<$command as $crate::__private::CommandDefinition>::descriptor()),*
                    ],
                    queries: ::std::vec![
                        <$first_query as $crate::__private::QueryDefinition>::descriptor(),
                        $(<$query as $crate::__private::QueryDefinition>::descriptor()),*
                    ],
                }
            }
        }
    };
    (
        $(#[$attribute:meta])*
        $visibility:vis struct $module:ident {
            commands: [$first:ty $(, $command:ty)* $(,)?] $(,)?
        }
    ) => {
        const _: () = {
            $(
                $crate::__private::assert_same_command_namespace(
                    <<$first as $crate::__private::CommandType>::Owner as
                        $crate::__private::CommandOwnerType>::COMMAND_NAMESPACE,
                    <<$command as $crate::__private::CommandType>::Owner as
                        $crate::__private::CommandOwnerType>::COMMAND_NAMESPACE,
                );
            )*
        };

        $(#[$attribute])*
        $visibility struct $module;

        impl $crate::__private::DomainModule for $module {
            const MODULE_NAME: &'static str =
                <<$first as $crate::__private::CommandType>::Owner as
                    $crate::__private::CommandOwnerType>::COMMAND_NAMESPACE;

            fn descriptor() -> $crate::__private::ModuleDescriptor {
                $crate::__private::ModuleDescriptor {
                    module_name: Self::MODULE_NAME,
                    commands: ::std::vec![
                        <$first as $crate::__private::CommandDefinition>::descriptor(),
                        $(<$command as $crate::__private::CommandDefinition>::descriptor()),*
                    ],
                    queries: ::std::vec![],
                }
            }
        }
    };
    (
        $(#[$attribute:meta])*
        $visibility:vis struct $module:ident {
            queries: [$first:ty $(, $query:ty)* $(,)?] $(,)?
        }
    ) => {
        const _: () = {
            $(
                $crate::__private::assert_same_module_namespace(
                    <$first as $crate::__private::QueryDefinition>::BOUNDED_CONTEXT,
                    <$query as $crate::__private::QueryDefinition>::BOUNDED_CONTEXT,
                );
            )*
        };

        $(#[$attribute])*
        $visibility struct $module;

        impl $crate::__private::DomainModule for $module {
            const MODULE_NAME: &'static str =
                <$first as $crate::__private::QueryDefinition>::BOUNDED_CONTEXT;

            fn descriptor() -> $crate::__private::ModuleDescriptor {
                $crate::__private::ModuleDescriptor {
                    module_name: Self::MODULE_NAME,
                    commands: ::std::vec![],
                    queries: ::std::vec![
                        <$first as $crate::__private::QueryDefinition>::descriptor(),
                        $(<$query as $crate::__private::QueryDefinition>::descriptor()),*
                    ],
                }
            }
        }
    };
}
