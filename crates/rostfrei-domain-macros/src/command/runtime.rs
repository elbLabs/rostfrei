use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;

pub fn assemble(runtime_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let owner = &attributes.owner;
    quote! {
        impl #runtime_path::__private::CommandDefinition for #name {
            type Aggregate = #owner;

            const COMMAND_NAME: &'static str =
                <Self as #runtime_path::__private::CommandType>::LOCAL_ID;
            const SCHEMA_VERSION: u32 =
                <Self as #runtime_path::__private::CommandType>::SCHEMA_VERSION;

            fn descriptor() -> #runtime_path::__private::CommandDescriptor {
                #runtime_path::__private::CommandDescriptor {
                    command_name: <Self as
                        #runtime_path::__private::CommandDefinition>::COMMAND_NAME,
                    schema_version: <Self as
                        #runtime_path::__private::CommandDefinition>::SCHEMA_VERSION,
                    aggregate_type: <Self::Aggregate as
                        #runtime_path::__private::Aggregate>::aggregate_type().into_owned(),
                    rust_command_type: #runtime_path::__private::type_name::<Self>(),
                    rust_aggregate_type:
                        #runtime_path::__private::type_name::<Self::Aggregate>(),
                    modeled_command: ::core::option::Option::Some(
                        <Self as #runtime_path::__private::CommandType>::DESCRIPTOR,
                    ),
                }
            }
        }
    }
}
