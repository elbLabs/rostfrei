use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use crate::field::Field;

use super::attributes::Attributes;

pub fn assemble(name: &Ident, attributes: &Attributes, fields: &[Field]) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let owner = &attributes.owner;
    let assertions = crate::field::assemble_assertions(name, None, fields);
    let fields = crate::field::assemble_descriptors(fields);

    quote! {
        impl ::rostfrei_domain::DomainEventType for #name {
            type Owner = #owner;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: ::rostfrei_domain::DomainEventDescriptor =
                ::rostfrei_domain::DomainEventDescriptor {
                    id: ::rostfrei_domain::DomainEventId {
                        aggregate: <#owner as ::rostfrei_domain::AggregateType>::DESCRIPTOR.id,
                        local: Self::LOCAL_ID,
                    },
                    label: #label,
                    fields: #fields,
                };
        }

        impl ::rostfrei_domain::ActionOutputType<
            ::rostfrei_domain::__private::AggregateActionOutput<#owner>
        > for #name {
            const DESCRIPTOR: Option<::rostfrei_domain::ActionOutputDescriptor> = Some(
                ::rostfrei_domain::ActionOutputDescriptor::DomainEvent(
                    <Self as ::rostfrei_domain::DomainEventType>::DESCRIPTOR.id,
                ),
            );
        }

        impl<Service> ::rostfrei_domain::ActionOutputType<
            ::rostfrei_domain::__private::DomainServiceActionOutput<Service>
        > for #name
        where
            Service: ::rostfrei_domain::DomainServiceType<
                Context = <#owner as ::rostfrei_domain::AggregateType>::Context,
            >,
        {
            const DESCRIPTOR: Option<::rostfrei_domain::ActionOutputDescriptor> = Some(
                ::rostfrei_domain::ActionOutputDescriptor::DomainEvent(
                    <Self as ::rostfrei_domain::DomainEventType>::DESCRIPTOR.id,
                ),
            );
        }

        #assertions
    }
}
