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
    let arguments = contract_arguments::parse(args)?;
    match arguments.kind {
        contract_arguments::ContractKind::Aggregate => {
            expand_aggregate_trait(item, arguments.instance_trait.as_ref())
        }
        contract_arguments::ContractKind::Entity => expand_entity_trait(item),
        contract_arguments::ContractKind::ValueObject => expand_value_object_trait(item),
        contract_arguments::ContractKind::DomainService => expand_domain_service_trait(item),
    }
}

fn expand_entity_trait(item: ItemTrait) -> syn::Result<TokenStream> {
    let mut item = trait_input::validate(item)?;
    let mut actions =
        trait_attributes::extract(&mut item.items, trait_attributes::RaisesPolicy::Forbidden)?;
    trait_validation::validate(&item.items, &mut actions)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    trait_assembly::assemble(&domain_path, item, &actions)
}

fn expand_value_object_trait(item: ItemTrait) -> syn::Result<TokenStream> {
    let mut item = trait_input::validate(item)?;
    let mut actions =
        trait_attributes::extract(&mut item.items, trait_attributes::RaisesPolicy::Forbidden)?;
    value_object_trait_validation::validate(&item.items, &mut actions)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    value_object_trait_assembly::assemble(&domain_path, item, &actions)
}

fn expand_aggregate_trait(
    item: ItemTrait,
    instance_trait: Option<&syn::Ident>,
) -> syn::Result<TokenStream> {
    let mut item = public_trait_input::validate(item, "aggregate")?;
    let raises_policy = if instance_trait.is_some() {
        trait_attributes::RaisesPolicy::Required
    } else {
        trait_attributes::RaisesPolicy::Forbidden
    };
    let mut actions = trait_attributes::extract(&mut item.items, raises_policy)?;
    if instance_trait.is_some() {
        aggregate_trait_validation::validate_instance(&item.items, &mut actions)?;
    } else {
        aggregate_trait_validation::validate(&item.items, &mut actions)?;
    }
    let domain_path = crate::helper::domain_api_path::resolve()?;
    let runtime_path = instance_trait
        .map(|_| crate::helper::runtime_api_path::resolve())
        .transpose()?;
    aggregate_trait_assembly::assemble(
        &domain_path,
        runtime_path.as_ref(),
        item,
        &actions,
        instance_trait,
    )
}

fn expand_domain_service_trait(item: ItemTrait) -> syn::Result<TokenStream> {
    let mut item = public_trait_input::validate(item, "domain service")?;
    let mut actions =
        trait_attributes::extract(&mut item.items, trait_attributes::RaisesPolicy::Forbidden)?;
    domain_service_trait_validation::validate(&item.items, &mut actions)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    domain_service_trait_assembly::assemble(&domain_path, item, &actions)
}
