use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::input::EventVariant;

pub fn assemble(domain_path: &Path, name: &Ident, variants: &[EventVariant]) -> TokenStream {
    let events: Vec<_> = variants.iter().map(|variant| &variant.event).collect();
    let field_types = events.iter().map(|event| {
        quote! {
            if id == <#event as #domain_path::DomainEventType<__RostfreiAggregate>>::DESCRIPTOR.id {
                return <#event as #domain_path::DomainEvent>::DECLARED_FIELD_TYPES;
            }
        }
    });
    let ownership = variants.iter().map(|variant| {
        let event = &variant.event;
        quote! {
            impl<__RostfreiAggregate> #domain_path::DomainEventType<__RostfreiAggregate>
                for #event
            where
                __RostfreiAggregate: #domain_path::AggregateDefinition<Event = #name>,
            {
            }
        }
    });

    quote! {
        #(#ownership)*

        impl<__RostfreiAggregate> #domain_path::AggregateEventSet<__RostfreiAggregate>
            for #name
        where
            __RostfreiAggregate: #domain_path::AggregateDefinition<Event = Self>,
        {
            const DOMAIN_EVENTS: &'static [#domain_path::DomainEventDescriptor] = &[
                #(<#events as #domain_path::DomainEventType<__RostfreiAggregate>>::DESCRIPTOR,)*
            ];

            fn declared_field_types(id: #domain_path::DomainEventId) -> &'static [#domain_path::DeclaredFieldType] {
                #(#field_types)*
                &[]
            }
        }
    }
}
