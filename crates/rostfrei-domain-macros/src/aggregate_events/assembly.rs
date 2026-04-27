use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::input::EventVariant;

pub fn assemble(domain_path: &Path, name: &Ident, variants: &[EventVariant]) -> TokenStream {
    let events = variants.iter().map(|variant| &variant.event);
    let ownership = variants.iter().map(|variant| {
        let event = &variant.event;
        quote! {
            impl<__RostfreiAggregate> #domain_path::DomainEventType<__RostfreiAggregate>
                for #event
            where
                __RostfreiAggregate: #domain_path::AggregateDefinition<Event = #name>,
            {
            }

            impl<__RostfreiAggregate> #domain_path::ActionOutputType<
                #domain_path::__private::AggregateActionOutput<__RostfreiAggregate>
            > for #event
            where
                __RostfreiAggregate: #domain_path::AggregateDefinition<Event = #name>,
            {
                const DESCRIPTOR: ::core::option::Option<
                    #domain_path::ActionOutputDescriptor
                > = ::core::option::Option::Some(
                    #domain_path::ActionOutputDescriptor::DomainEvent(
                        <Self as #domain_path::DomainEventType<
                            __RostfreiAggregate
                        >>::DESCRIPTOR.id,
                    ),
                );
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
        }
    }
}
