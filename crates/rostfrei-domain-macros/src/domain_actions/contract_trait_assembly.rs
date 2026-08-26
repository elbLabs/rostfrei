use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{ItemTrait, Path, TraitItem, Type, TypeParamBound, WherePredicate};

use super::action::Action;
use super::action_reference;

type OwnerPredicateAssembler = fn(&mut ItemTrait, &Action) -> syn::Result<()>;

pub enum OutputPolicy {
    Declared(Path),
    OwnerSelf(Path),
}

pub struct Configuration {
    pub owner_supertrait: TypeParamBound,
    pub output_policy: OutputPolicy,
    pub owner_predicate: Option<OwnerPredicateAssembler>,
}

pub fn assemble(
    mut item: ItemTrait,
    actions: &[Action],
    configuration: Configuration,
) -> syn::Result<TokenStream> {
    add_supertraits(&mut item, configuration.owner_supertrait);
    for action in actions {
        if let Some(assemble_owner_predicate) = configuration.owner_predicate {
            assemble_owner_predicate(&mut item, action)?;
        }
        add_action_predicates(&mut item, action, &configuration.output_policy)?;
    }
    action_reference::add(&mut item, actions)?;
    add_action_descriptors(&mut item, actions, &configuration.output_policy)?;
    add_domain_actions_attribute_requirement(&mut item)?;
    Ok(quote!(#item))
}

fn add_supertraits(item: &mut ItemTrait, owner_supertrait: TypeParamBound) {
    item.supertraits.push(owner_supertrait);
    item.supertraits.push(syn::parse_quote!(Sized));
}

fn add_action_predicates(
    item: &mut ItemTrait,
    action: &Action,
    output_policy: &OutputPolicy,
) -> syn::Result<()> {
    let signature = action.signature.as_ref().unwrap();
    if let Some(input) = &signature.input {
        push_predicate(item, input, quote!(::domain::ActionInputType<Self>))?;
    }
    match output_policy {
        OutputPolicy::Declared(output_owner) => push_predicate(
            item,
            &signature.output,
            quote!(::domain::ActionOutputType<#output_owner<Self>>),
        )?,
        OutputPolicy::OwnerSelf(output_owner) => {
            push_predicate(
                item,
                &signature.output,
                quote!(::domain::__private::SameType<Type = Self>),
            )?;
            let predicate: WherePredicate = syn::parse2(quote! {
                Self: ::domain::ActionOutputType<#output_owner<Self>>
            })?;
            item.generics.make_where_clause().predicates.push(predicate);
        }
    }
    if let Some(error) = &signature.error {
        push_predicate(item, error, quote!(::domain::DomainErrorType<Owner = Self>))?;
    }
    Ok(())
}

fn push_predicate(item: &mut ItemTrait, ty: &Type, bound: TokenStream) -> syn::Result<()> {
    let predicate: WherePredicate = syn::parse2(quote_spanned! {ty.span()=> #ty: #bound})?;
    item.generics.make_where_clause().predicates.push(predicate);
    Ok(())
}

fn add_action_descriptors(
    item: &mut ItemTrait,
    actions: &[Action],
    output_policy: &OutputPolicy,
) -> syn::Result<()> {
    let descriptors = actions
        .iter()
        .map(|action| assemble_descriptor(action, output_policy));
    let span = item.ident.span();
    let constant: TraitItem = syn::parse2(quote_spanned! {span=>
        #[doc(hidden)]
        const __DOMAIN_ACTIONS: &'static [::domain::ActionDescriptor] = &[
            #(#descriptors),*
        ];
    })?;
    item.items.push(constant);
    Ok(())
}

fn add_domain_actions_attribute_requirement(item: &mut ItemTrait) -> syn::Result<()> {
    let span = item.ident.span();
    let constant: TraitItem = syn::parse2(quote_spanned! {span=>
        #[doc(hidden)]
        const __DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE: &'static [
            ::domain::ActionDescriptor
        ] = Self::__DOMAIN_ACTIONS;
    })?;
    item.items.push(constant);
    Ok(())
}

fn assemble_descriptor(action: &Action, output_policy: &OutputPolicy) -> TokenStream {
    let id = &action.id;
    let label = &action.label;
    let signature = action.signature.as_ref().unwrap();
    let input = signature.input.as_ref().map_or_else(
        || quote!(None),
        |input| quote!(Some(<#input as ::domain::ActionInputType<Self>>::DESCRIPTOR)),
    );
    let declared_output = &signature.output;
    let (output, output_owner) = match output_policy {
        OutputPolicy::Declared(output_owner) => (quote!(#declared_output), output_owner),
        OutputPolicy::OwnerSelf(output_owner) => (quote!(Self), output_owner),
    };
    let error = signature.error.as_ref().map_or_else(
        || quote!(None),
        |error| quote!(Some(<#error as ::domain::DomainErrorType>::DESCRIPTOR.id)),
    );
    quote! {
        ::domain::ActionDescriptor {
            id: ::domain::ActionId {
                owner: <Self as ::domain::ActionOwnerType>::ACTION_OWNER_ID,
                local: #id,
            },
            label: #label,
            input: #input,
            output: <#output as ::domain::ActionOutputType<
                #output_owner<Self>
            >>::DESCRIPTOR,
            error: #error,
        }
    }
}
