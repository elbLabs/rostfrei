use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Ident, Path, TypePath, Visibility};

#[allow(
    clippy::too_many_lines,
    reason = "keeps aggregate runtime token generation and generated item ordering in one auditable block"
)]
pub fn assemble(
    domain_path: &Path,
    runtime_path: &Path,
    name: &Ident,
    visibility: &Visibility,
    root: &TypePath,
    events: &[Path],
) -> TokenStream {
    let event_enum = format_ident!("__{name}Event");
    let variants: Vec<_> = (0..events.len())
        .map(|index| format_ident!("Event{index}"))
        .collect();

    let conversions = events.iter().zip(&variants).map(|(event, variant)| {
        quote! {
            impl ::core::convert::From<#event> for #event_enum {
                fn from(event: #event) -> Self {
                    Self::#variant(event)
                }
            }

            impl #runtime_path::__private::core::EventVariant<#event> for #event_enum {
                fn event(&self) -> ::core::option::Option<&#event> {
                    match self {
                        Self::#variant(event) => ::core::option::Option::Some(event),
                        _ => ::core::option::Option::None,
                    }
                }

                fn into_event(self) -> ::core::option::Option<#event> {
                    match self {
                        Self::#variant(event) => ::core::option::Option::Some(event),
                        _ => ::core::option::Option::None,
                    }
                }
            }
        }
    });

    quote! {
        const _: () = #runtime_path::__private::assert_unique_event_ids(&[
            #(<#events as #domain_path::DomainEventDefinitionType>::DEFINITION.id,)*
        ]);

        #[doc(hidden)]
        #visibility enum #event_enum {
            #(#variants(#events),)*
        }

        #(#conversions)*

        impl #runtime_path::__private::core::Event for #event_enum {
            fn event_type(&self) -> &'static str {
                match self {
                    #(
                        Self::#variants(_) =>
                            <#events as #domain_path::DomainEventDefinitionType>::DEFINITION.id,
                    )*
                }
            }

            fn schema_version(&self) -> u32 {
                match self {
                    #(
                        Self::#variants(_) =>
                            <#events as #domain_path::DomainEventDefinitionType>::DEFINITION.schema_version,
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
                        Self::#variants(event) =>
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
                        == <#events as #domain_path::DomainEventDefinitionType>::DEFINITION.id
                    {
                        let expected =
                            <#events as #domain_path::DomainEventDefinitionType>::DEFINITION.schema_version;
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
                        .map(Self::#variants);
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

        impl #runtime_path::__private::core::Aggregate for #name {
            type State = #root;
            type Event = #event_enum;

            const AGGREGATE_TYPE: &'static str =
                <Self as #domain_path::AggregateType>::DESCRIPTOR.id.local;

            fn aggregate_type() -> ::std::borrow::Cow<'static, str> {
                let id = <Self as #domain_path::AggregateType>::DESCRIPTOR.id;
                ::std::borrow::Cow::Owned(
                    ::std::format!("{}/{}", id.context.0, id.local),
                )
            }

            fn initial(
                stream_id: &#runtime_path::__private::core::StreamId,
            ) -> Self::State {
                <#root as #runtime_path::Initialize<Self>>::initialize(stream_id)
            }

            fn apply(state: &mut Self::State, event: &Self::Event) {
                match event {
                    #(
                        #event_enum::#variants(event) =>
                            <#root as #runtime_path::Apply<#events>>::apply(state, event),
                    )*
                }
            }
        }

        impl #runtime_path::AggregateRuntime for #name {}
    }
}
