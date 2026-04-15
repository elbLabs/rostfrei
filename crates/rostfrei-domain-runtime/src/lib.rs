use domain::AggregateType;
use rostfrei_core::Aggregate;

/// Applies one concrete domain event to an aggregate root.
pub trait Apply<Event> {
    fn apply(&mut self, event: &Event);
}

/// Constructs an aggregate root for a stream before its history is replayed.
pub trait Initialize<A: AggregateType>: Sized {
    fn initialize(stream_id: &rostfrei_core::StreamId) -> Self;
}

/// Marks a compiled aggregate whose descriptor and executable definition agree.
pub trait AggregateRuntime:
    AggregateType + Aggregate<State = <Self as AggregateType>::Root>
{
}

#[doc(hidden)]
pub mod __private {
    pub use domain::{DomainCommandOwnerType, DomainCommandType};
    pub use rostfrei_core as core;
    pub use rostfrei_core::{Aggregate, AggregateInstance, CommandHandler};
    pub use rostfrei_registry::{
        CommandDefinition, CommandDescriptor, DomainModule, ModuleDescriptor,
    };
    pub use std::any::type_name;

    pub use crate::{AggregateRuntime, Apply, Initialize};

    pub const fn assert_unique_event_ids(ids: &[&str]) {
        let mut left = 0;
        while left < ids.len() {
            let mut right = left + 1;
            while right < ids.len() {
                assert!(
                    !strings_equal(ids[left], ids[right]),
                    "duplicate domain event ID in aggregate"
                );
                right += 1;
            }
            left += 1;
        }
    }

    pub const fn assert_same_command_namespace(left: &str, right: &str) {
        assert!(
            strings_equal(left, right),
            "domain module commands must belong to the same namespace"
        );
    }

    const fn strings_equal(left: &str, right: &str) -> bool {
        let left = left.as_bytes();
        let right = right.as_bytes();
        if left.len() != right.len() {
            return false;
        }
        let mut index = 0;
        while index < left.len() {
            if left[index] != right[index] {
                return false;
            }
            index += 1;
        }
        true
    }
}

/// Generates rostfrei command and module registrations from domain command types.
///
/// The generated form derives ownership, local wire names, schema versions, and
/// structural metadata from the domain model. The explicit form overrides the
/// module name and command wire identities.
#[macro_export]
macro_rules! domain_module {
    (
        $(#[$attribute:meta])*
        $visibility:vis struct $module:ident {
            commands: [$first:ty $(, $command:ty)* $(,)?] $(,)?
        }
    ) => {
        impl $crate::__private::CommandDefinition for $first {
            type Aggregate = <$first as $crate::__private::DomainCommandType>::Owner;

            const COMMAND_NAME: &'static str =
                <$first as $crate::__private::DomainCommandType>::LOCAL_ID;
            const SCHEMA_VERSION: u32 =
                <$first as $crate::__private::DomainCommandType>::SCHEMA_VERSION;

            fn descriptor() -> $crate::__private::CommandDescriptor {
                $crate::__private::CommandDescriptor {
                    command_name: <Self as $crate::__private::CommandDefinition>::COMMAND_NAME,
                    schema_version: <Self as $crate::__private::CommandDefinition>::SCHEMA_VERSION,
                    aggregate_type: <Self::Aggregate as $crate::__private::Aggregate>::aggregate_type().into_owned(),
                    rust_command_type: $crate::__private::type_name::<Self>(),
                    rust_aggregate_type: $crate::__private::type_name::<Self::Aggregate>(),
                    domain_command: ::core::option::Option::Some(
                        <Self as $crate::__private::DomainCommandType>::DESCRIPTOR,
                    ),
                }
            }
        }

        $(
            impl $crate::__private::CommandDefinition for $command {
                type Aggregate = <$command as $crate::__private::DomainCommandType>::Owner;

                const COMMAND_NAME: &'static str =
                    <$command as $crate::__private::DomainCommandType>::LOCAL_ID;
                const SCHEMA_VERSION: u32 =
                    <$command as $crate::__private::DomainCommandType>::SCHEMA_VERSION;

                fn descriptor() -> $crate::__private::CommandDescriptor {
                    $crate::__private::CommandDescriptor {
                        command_name: <Self as $crate::__private::CommandDefinition>::COMMAND_NAME,
                        schema_version: <Self as $crate::__private::CommandDefinition>::SCHEMA_VERSION,
                        aggregate_type: <Self::Aggregate as $crate::__private::Aggregate>::aggregate_type().into_owned(),
                        rust_command_type: $crate::__private::type_name::<Self>(),
                        rust_aggregate_type: $crate::__private::type_name::<Self::Aggregate>(),
                        domain_command: ::core::option::Option::Some(
                            <Self as $crate::__private::DomainCommandType>::DESCRIPTOR,
                        ),
                    }
                }
            }
        )*

        const _: () = {
            $(
                $crate::__private::assert_same_command_namespace(
                    <<$first as $crate::__private::DomainCommandType>::Owner as
                        $crate::__private::DomainCommandOwnerType>::DOMAIN_COMMAND_NAMESPACE,
                    <<$command as $crate::__private::DomainCommandType>::Owner as
                        $crate::__private::DomainCommandOwnerType>::DOMAIN_COMMAND_NAMESPACE,
                );
            )*
        };

        $(#[$attribute])*
        $visibility struct $module;

        impl $crate::__private::DomainModule for $module {
            const MODULE_NAME: &'static str =
                <<$first as $crate::__private::DomainCommandType>::Owner as
                    $crate::__private::DomainCommandOwnerType>::DOMAIN_COMMAND_NAMESPACE;

            fn descriptor() -> $crate::__private::ModuleDescriptor {
                $crate::__private::ModuleDescriptor {
                    module_name: Self::MODULE_NAME,
                    commands: ::std::vec![
                        <$first as $crate::__private::CommandDefinition>::descriptor(),
                        $(<$command as $crate::__private::CommandDefinition>::descriptor()),*
                    ],
                }
            }
        }
    };

    (
        $(#[$attribute:meta])*
        $visibility:vis struct $module:ident {
            name: $module_name:literal,
            commands: [
                $(
                    $command:ty => {
                        name: $command_name:literal,
                        version: $schema_version:literal $(,)?
                    }
                ),+ $(,)?
            ] $(,)?
        }
    ) => {
        $(
            impl $crate::__private::CommandDefinition for $command {
                type Aggregate = <$command as $crate::__private::DomainCommandType>::Owner;

                const COMMAND_NAME: &'static str = $command_name;
                const SCHEMA_VERSION: u32 = $schema_version;

                fn descriptor() -> $crate::__private::CommandDescriptor {
                    $crate::__private::CommandDescriptor {
                        command_name: <Self as $crate::__private::CommandDefinition>::COMMAND_NAME,
                        schema_version: <Self as $crate::__private::CommandDefinition>::SCHEMA_VERSION,
                        aggregate_type: <Self::Aggregate as $crate::__private::Aggregate>::aggregate_type().into_owned(),
                        rust_command_type: $crate::__private::type_name::<Self>(),
                        rust_aggregate_type: $crate::__private::type_name::<Self::Aggregate>(),
                        domain_command: ::core::option::Option::Some(
                            <Self as $crate::__private::DomainCommandType>::DESCRIPTOR,
                        ),
                    }
                }
            }
        )+

        $(#[$attribute])*
        $visibility struct $module;

        impl $crate::__private::DomainModule for $module {
            const MODULE_NAME: &'static str = $module_name;

            fn descriptor() -> $crate::__private::ModuleDescriptor {
                $crate::__private::ModuleDescriptor {
                    module_name: Self::MODULE_NAME,
                    commands: ::std::vec![
                        $(<$command as $crate::__private::CommandDefinition>::descriptor()),+
                    ],
                }
            }
        }
    };
}

/// Connects a domain command to an aggregate-instance action method.
#[macro_export]
macro_rules! domain_command_handler {
    ($command:ty => $method:ident) => {
        impl $crate::__private::CommandHandler<$command>
            for <$command as $crate::__private::DomainCommandType>::Owner
        {
            type Rejection = <$command as $crate::__private::DomainCommandType>::Rejection;

            fn handle(
                command: &$command,
                aggregate: &mut $crate::__private::AggregateInstance<Self>,
            ) -> ::core::result::Result<(), Self::Rejection> {
                aggregate.$method(command)
            }
        }
    };
}
