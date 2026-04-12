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
        impl ::domain::DomainEventType for #name {
            type Owner = #owner;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: ::domain::DomainEventDescriptor =
                ::domain::DomainEventDescriptor {
                    id: ::domain::DomainEventId {
                        aggregate: <#owner as ::domain::AggregateType>::DESCRIPTOR.id,
                        local: Self::LOCAL_ID,
                    },
                    label: #label,
                    fields: #fields,
                };
        }

        impl ::domain::ActionOutputType<
            ::domain::__private::AggregateActionOutput<#owner>
        > for #name {
            const DESCRIPTOR: Option<::domain::ActionOutputDescriptor> = Some(
                ::domain::ActionOutputDescriptor::DomainEvent(
                    <Self as ::domain::DomainEventType>::DESCRIPTOR.id,
                ),
            );
        }

        impl<Service> ::domain::ActionOutputType<
            ::domain::__private::DomainServiceActionOutput<Service>
        > for #name
        where
            Service: ::domain::DomainServiceType<
                Context = <#owner as ::domain::AggregateType>::Context,
            >,
        {
            const DESCRIPTOR: Option<::domain::ActionOutputDescriptor> = Some(
                ::domain::ActionOutputDescriptor::DomainEvent(
                    <Self as ::domain::DomainEventType>::DESCRIPTOR.id,
                ),
            );
        }

        #assertions
    }
}
