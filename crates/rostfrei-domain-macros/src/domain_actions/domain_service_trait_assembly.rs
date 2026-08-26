use proc_macro2::TokenStream;
use syn::ItemTrait;

use super::action::Action;
use super::contract_trait_assembly::{self, Configuration, OutputPolicy};

pub fn assemble(item: ItemTrait, actions: &[Action]) -> syn::Result<TokenStream> {
    contract_trait_assembly::assemble(
        item,
        actions,
        Configuration {
            owner_supertrait: syn::parse_quote!(::domain::DomainServiceActionOwnerType),
            output_policy: OutputPolicy::Declared(syn::parse_quote!(
                ::domain::__private::DomainServiceActionOutput
            )),
            owner_predicate: None,
        },
    )
}
