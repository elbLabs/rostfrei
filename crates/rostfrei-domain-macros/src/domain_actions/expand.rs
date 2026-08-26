use proc_macro2::TokenStream;
use syn::{Item, ItemTrait};

use super::{
    aggregate_trait_assembly, aggregate_trait_validation, contract_arguments,
    domain_service_trait_assembly, domain_service_trait_validation, public_trait_input,
    trait_assembly, trait_attributes, trait_input, trait_validation, value_object_trait_assembly,
    value_object_trait_validation,
};

pub fn expand(args: TokenStream, tokens: TokenStream) -> syn::Result<TokenStream> {
    match syn::parse2(tokens)? {
        Item::Trait(item) => expand_trait(args, item),
        item => Err(syn::Error::new_spanned(
            item,
            "domain_actions may only be applied to a trait",
        )),
    }
}

fn expand_trait(args: TokenStream, item: ItemTrait) -> syn::Result<TokenStream> {
    match contract_arguments::parse(args)? {
        contract_arguments::ContractKind::Aggregate => expand_aggregate_trait(item),
        contract_arguments::ContractKind::Entity => expand_entity_trait(item),
        contract_arguments::ContractKind::ValueObject => expand_value_object_trait(item),
        contract_arguments::ContractKind::DomainService => expand_domain_service_trait(item),
    }
}

fn expand_entity_trait(item: ItemTrait) -> syn::Result<TokenStream> {
    let mut item = trait_input::validate(item)?;
    let mut actions = trait_attributes::extract(&mut item.items)?;
    trait_validation::validate(&item.items, &mut actions)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    trait_assembly::assemble(&domain_path, item, &actions)
}

fn expand_value_object_trait(item: ItemTrait) -> syn::Result<TokenStream> {
    let mut item = trait_input::validate(item)?;
    let mut actions = trait_attributes::extract(&mut item.items)?;
    value_object_trait_validation::validate(&item.items, &mut actions)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    value_object_trait_assembly::assemble(&domain_path, item, &actions)
}

fn expand_aggregate_trait(item: ItemTrait) -> syn::Result<TokenStream> {
    let mut item = public_trait_input::validate(item, "aggregate")?;
    let mut actions = trait_attributes::extract(&mut item.items)?;
    aggregate_trait_validation::validate(&item.items, &mut actions)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    aggregate_trait_assembly::assemble(&domain_path, item, &actions)
}

fn expand_domain_service_trait(item: ItemTrait) -> syn::Result<TokenStream> {
    let mut item = public_trait_input::validate(item, "domain service")?;
    let mut actions = trait_attributes::extract(&mut item.items)?;
    domain_service_trait_validation::validate(&item.items, &mut actions)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    domain_service_trait_assembly::assemble(&domain_path, item, &actions)
}
