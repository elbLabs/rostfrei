use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, ItemTrait, Path};

use super::action::Action;

pub fn assemble(
    domain_path: &Path,
    runtime_path: &Path,
    contract: &ItemTrait,
    actions: &[Action],
    instance_trait: &Ident,
) -> TokenStream {
    let visibility = &contract.vis;
    let contract_name = &contract.ident;
    let methods = actions.iter().map(|action| {
        let signature = &action.syntax;
        quote! {
            #signature;
        }
    });
    let action_bounds = actions.iter().map(|action| {
        let signature = action.signature.as_ref().unwrap();
        let input = signature
            .input
            .as_ref()
            .map(|input| quote!(#input: #domain_path::ActionInputType<__RostfreiAggregate>,));
        let error = signature.error.as_ref().map(
            |error| quote!(#error: #domain_path::DomainErrorType<Owner = __RostfreiAggregate>,),
        );
        let events = action.raises.iter().map(|event| {
            quote! {
                #event: #domain_path::DomainEventType<Owner = __RostfreiAggregate>
                    + ::core::convert::Into<
                        <__RostfreiAggregate as #runtime_path::__private::Aggregate>::Event
                    >,
            }
        });
        quote! {
            #input
            #error
            #(#events)*
        }
    });

    quote! {
        #visibility trait #instance_trait {
            #(#methods)*
        }

        impl<__RostfreiAggregate> #contract_name for __RostfreiAggregate
        where
            __RostfreiAggregate: #domain_path::AggregateActionOwnerType
                + #runtime_path::AggregateRuntime,
            #runtime_path::__private::AggregateInstance<__RostfreiAggregate>: #instance_trait,
            #(#action_bounds)*
        {}
    }
}
