use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

pub fn assemble(domain_path: &Path, name: &Ident, events: &[Path]) -> TokenStream {
    let ownership = events.iter().map(|event| {
        quote! {
            impl #domain_path::DomainEventType for #event {
                type Owner = #name;
            }

            impl #domain_path::ActionOutputType<
                #domain_path::__private::AggregateActionOutput<#name>
            > for #event {
                const DESCRIPTOR: ::core::option::Option<#domain_path::ActionOutputDescriptor> =
                    ::core::option::Option::Some(
                        #domain_path::ActionOutputDescriptor::DomainEvent(
                            <Self as #domain_path::DomainEventType>::DESCRIPTOR.id,
                        ),
                    );
            }

            impl<Service> #domain_path::ActionOutputType<
                #domain_path::__private::DomainServiceActionOutput<Service>
            > for #event
            where
                Service: #domain_path::DomainServiceType<
                    Context = <#name as #domain_path::AggregateType>::Context,
                >,
            {
                const DESCRIPTOR: ::core::option::Option<#domain_path::ActionOutputDescriptor> =
                    ::core::option::Option::Some(
                        #domain_path::ActionOutputDescriptor::DomainEvent(
                            <Self as #domain_path::DomainEventType>::DESCRIPTOR.id,
                        ),
                    );
            }
        }
    });

    quote!(#(#ownership)*)
}
