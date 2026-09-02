use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{ToTokens as _, quote};
use syn::{Ident, ItemTrait, Path};

use super::action::Action;
use super::signature::ParsedSignature;

struct AssembledAction<'a> {
    action: &'a Action,
    signature: &'a ParsedSignature,
}

pub fn assemble(
    domain_path: &Path,
    runtime_path: &Path,
    contract: &ItemTrait,
    actions: &[Action],
    instance_trait: &Ident,
) -> syn::Result<TokenStream> {
    let actions = validate_actions(actions)?;
    let visibility = &contract.vis;
    let contract_name = &contract.ident;
    let methods = actions.iter().map(|action| {
        let signature = &action.action.syntax;
        quote! {
            #signature;
        }
    });
    let action_bounds = actions.iter().map(|action| {
        let signature = action.signature;
        let error = signature.error.as_ref().map(
            |error| quote!(#error: #domain_path::DomainErrorType<Owner = __RostfreiAggregate>,),
        );
        quote! {
            #error
        }
    });
    let mut event_keys = HashSet::new();
    let event_bounds = actions
        .iter()
        .flat_map(|action| &action.action.raises)
        .filter(|event| event_keys.insert(event.to_token_stream().to_string()))
        .map(|event| {
            quote! {
                #event: #domain_path::DomainEventType<__RostfreiAggregate>
                    + ::core::convert::Into<
                        <__RostfreiAggregate as #runtime_path::__private::Aggregate>::Event
                    >,
            }
        });

    Ok(quote! {
        #visibility trait #instance_trait {
            #(#methods)*
        }

        impl<__RostfreiAggregate> #contract_name for __RostfreiAggregate
        where
            __RostfreiAggregate: #domain_path::AggregateActionOwnerType
                + #runtime_path::AggregateRuntime,
            #runtime_path::__private::AggregateInstance<__RostfreiAggregate>: #instance_trait,
            #(#action_bounds)*
            #(#event_bounds)*
        {}
    })
}

fn validate_actions(actions: &[Action]) -> syn::Result<Vec<AssembledAction<'_>>> {
    actions
        .iter()
        .map(|action| {
            let Some(signature) = action.signature.as_ref() else {
                return Err(syn::Error::new_spanned(
                    &action.syntax,
                    "domain action signature must be validated before assembly",
                ));
            };
            Ok(AssembledAction { action, signature })
        })
        .collect()
}
