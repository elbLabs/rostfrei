use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

pub fn assemble(domain_path: &Path, runtime_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #runtime_path::__private::core::Aggregate for #name {
            type State = <Self as #domain_path::AggregateDefinition>::Root;
            type Event = <Self as #domain_path::AggregateDefinition>::Event;

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
                <Self::State as #runtime_path::Initialize<Self>>::initialize(stream_id)
            }

            fn apply(state: &mut Self::State, event: &Self::Event) {
                <Self::Event as #runtime_path::AggregateEventRuntime<Self>>::apply(state, event)
            }
        }

        impl #runtime_path::AggregateRuntime for #name {}
    }
}
