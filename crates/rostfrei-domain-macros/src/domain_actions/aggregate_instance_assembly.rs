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
        let method = &action.syntax.ident;
        let signature = action.signature.as_ref().unwrap();
        let input = signature
            .input
            .as_ref()
            .map(|input| quote!(, input: #input));
        let output = signature.error.as_ref().map_or_else(
            || quote!(()),
            |error| quote!(::core::result::Result<(), #error>),
        );
        quote! {
            fn #method(&mut self #input) -> #output;
        }
    });
    let implementations = actions.iter().map(|action| {
        let method = &action.syntax.ident;
        let signature = action.signature.as_ref().unwrap();
        let input = signature
            .input
            .as_ref()
            .map(|input| quote!(, input: #input));
        let argument = signature.input.as_ref().map(|_| quote!(, input));
        if let Some(error) = &signature.error {
            quote! {
                fn #method(
                    &mut self #input,
                ) -> ::core::result::Result<(), #error> {
                    let event = <__RostfreiAggregate as #contract_name>::#method(
                        self.state() #argument,
                    )?;
                    self.raise(event);
                    ::core::result::Result::Ok(())
                }
            }
        } else {
            quote! {
                fn #method(&mut self #input) {
                    let event = <__RostfreiAggregate as #contract_name>::#method(
                        self.state() #argument,
                    );
                    self.raise(event);
                }
            }
        }
    });
    let event_types = actions
        .iter()
        .map(|action| &action.signature.as_ref().unwrap().output);
    let action_bounds = actions.iter().map(|action| {
        let signature = action.signature.as_ref().unwrap();
        let root = signature.root.as_ref().unwrap();
        let input = signature
            .input
            .as_ref()
            .map(|input| quote!(#input: #domain_path::ActionInputType<__RostfreiAggregate>,));
        let output = &signature.output;
        let error = signature.error.as_ref().map(
            |error| quote!(#error: #domain_path::DomainErrorType<Owner = __RostfreiAggregate>,),
        );
        quote! {
            __RostfreiAggregate: #domain_path::AggregateType<Root = #root>,
            #input
            #output: #domain_path::ActionOutputType<
                #domain_path::__private::AggregateActionOutput<__RostfreiAggregate>
            > + #domain_path::DomainEventType<Owner = __RostfreiAggregate>,
            #error
        }
    });

    quote! {
        #visibility trait #instance_trait {
            #(#methods)*
        }

        impl<__RostfreiAggregate> #instance_trait
            for #runtime_path::__private::AggregateInstance<__RostfreiAggregate>
        where
            __RostfreiAggregate: #runtime_path::AggregateRuntime + #contract_name,
            #(#action_bounds)*
            #(
                #event_types: ::core::convert::Into<
                    <__RostfreiAggregate as #runtime_path::__private::Aggregate>::Event
                >,
            )*
        {
            #(#implementations)*
        }
    }
}
