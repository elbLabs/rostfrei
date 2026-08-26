use domain::AggregateType;
use rostfrei_core::Aggregate;

/// Connects a descriptive domain aggregate to its executable rostfrei state.
pub trait AggregateRuntime: AggregateType {
    type Runtime: Aggregate;
}

#[doc(hidden)]
pub mod __private {
    pub use domain::DomainCommandType;
    pub use rostfrei_core::Aggregate;
    pub use rostfrei_registry::{
        CommandDefinition, CommandDescriptor, DomainModule, ModuleDescriptor,
    };
    pub use std::any::type_name;

    pub use crate::AggregateRuntime;
}

/// Generates rostfrei command and module registrations from domain command types.
///
/// The domain model supplies command ownership and structural metadata. The binding
/// supplies only runtime wire names and schema versions.
#[macro_export]
macro_rules! domain_module {
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
                type Aggregate = <<$command as $crate::__private::DomainCommandType>::Owner as $crate::AggregateRuntime>::Runtime;

                const COMMAND_NAME: &'static str = $command_name;
                const SCHEMA_VERSION: u32 = $schema_version;

                fn descriptor() -> $crate::__private::CommandDescriptor {
                    $crate::__private::CommandDescriptor {
                        command_name: Self::COMMAND_NAME,
                        schema_version: Self::SCHEMA_VERSION,
                        aggregate_type: <Self::Aggregate as $crate::__private::Aggregate>::AGGREGATE_TYPE,
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
