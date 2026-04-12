use proc_macro2::TokenStream;
use syn::{ItemTrait, Path};

use super::action::Action;
use super::contract_trait_assembly::{self, Configuration, OutputPolicy};

pub fn assemble(
    domain_path: &Path,
    item: ItemTrait,
    actions: &[Action],
) -> syn::Result<TokenStream> {
    contract_trait_assembly::assemble(
        domain_path,
        item,
        actions,
        Configuration {
            owner_supertrait: syn::parse_quote!(#domain_path::ValueObjectActionOwnerType),
            output_policy: OutputPolicy::OwnerSelf(syn::parse_quote!(
                #domain_path::__private::ValueObjectActionOutput
            )),
            owner_predicate: None,
        },
    )
}
