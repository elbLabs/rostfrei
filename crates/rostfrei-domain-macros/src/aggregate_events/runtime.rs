use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::input::EventVariant;

#[allow(
    clippy::too_many_lines,
    reason = "keeps the generated event codec and dispatch behavior auditable"
)]
pub fn assemble(
    domain_path: &Path,
    runtime_path: &Path,
    name: &Ident,
    variants: &[EventVariant],
) -> TokenStream {
    let variant_names: Vec<_> = variants.iter().map(|variant| &variant.name).collect();
    let events: Vec<_> = variants.iter().map(|variant| &variant.event).collect();
    let conversions = variants.iter().map(|variant| {
        let variant_name = &variant.name;
        let event = &variant.event;
        quote! {
            impl ::core::convert::From<#event> for #name {
                fn from(event: #event) -> Self {
                    Self::#variant_name(event)
                }
            }

            impl #runtime_path::__private::core::EventVariant<#event> for #name {
                fn event(&self) -> ::core::option::Option<&#event> {
                    match self {
                        Self::#variant_name(event) => ::core::option::Option::Some(event),
                        _ => ::core::option::Option::None,
                    }
                }

                fn into_event(self) -> ::core::option::Option<#event> {
                    match self {
                        Self::#variant_name(event) => ::core::option::Option::Some(event),
                        _ => ::core::option::Option::None,
                    }
                }
            }
        }
    });
    let apply_bounds = events.iter().map(|event| {
        quote! {
            <__RostfreiAggregate as #domain_path::AggregateDefinition>::Root:
                #runtime_path::Apply<#event>,
        }
    });

    quote! {
        const _: () = #runtime_path::__private::assert_unique_event_ids(&[
            #(<#events as #domain_path::DomainEvent>::LOCAL_ID,)*
        ]);

        #(#conversions)*

        impl #runtime_path::__private::core::Event for #name {
            fn event_type(&self) -> &'static str {
                match self {
                    #(
                        Self::#variant_names(_) =>
                            <#events as #domain_path::DomainEvent>::LOCAL_ID,
                    )*
                }
            }

            fn schema_version(&self) -> u32 {
                match self {
                    #(
                        Self::#variant_names(_) =>
                            <#events as #domain_path::DomainEvent>::SCHEMA_VERSION,
                    )*
                }
            }

            fn encode_json(
                &self,
            ) -> ::core::result::Result<
                ::std::vec::Vec<u8>,
                #runtime_path::__private::core::EventCodecError,
            > {
                match self {
                    #(
                        Self::#variant_names(event) =>
                            #runtime_path::__private::core::__private::encode_json(event),
                    )*
                }
            }

            fn decode_json(
                recorded: &#runtime_path::__private::core::RecordedEvent,
            ) -> ::core::result::Result<
                Self,
                #runtime_path::__private::core::EventCodecError,
            > {
                #(
                    if recorded.event_type()
                        == <#events as #domain_path::DomainEvent>::LOCAL_ID
                    {
                        let expected =
                            <#events as #domain_path::DomainEvent>::SCHEMA_VERSION;
                        if recorded.schema_version() != expected {
                            return ::core::result::Result::Err(
                                #runtime_path::__private::core::EventCodecError::new(
                                    #runtime_path::__private::core::EventCodecErrorKind::UnsupportedSchemaVersion,
                                    ::std::format!(
                                        "event type {} supports schema version {}, not {}",
                                        recorded.event_type(),
                                        expected,
                                        recorded.schema_version(),
                                    ),
                                ),
                            );
                        }
                        return #runtime_path::__private::core::__private::decode_json::<#events>(
                            recorded.payload(),
                        )
                        .map(Self::#variant_names);
                    }
                )*

                ::core::result::Result::Err(
                    #runtime_path::__private::core::EventCodecError::new(
                        #runtime_path::__private::core::EventCodecErrorKind::UnknownEventType,
                        ::std::format!("unknown event type {}", recorded.event_type()),
                    ),
                )
            }
        }

        impl<__RostfreiAggregate> #runtime_path::AggregateEventRuntime<__RostfreiAggregate>
            for #name
        where
            __RostfreiAggregate: #domain_path::AggregateDefinition<Event = Self>,
            #(#apply_bounds)*
        {
            fn apply(
                root: &mut <__RostfreiAggregate as #domain_path::AggregateDefinition>::Root,
                event: &Self,
            ) {
                match event {
                    #(
                        Self::#variant_names(event) =>
                            #runtime_path::Apply::<#events>::apply(root, event),
                    )*
                }
            }
        }
    }
}
