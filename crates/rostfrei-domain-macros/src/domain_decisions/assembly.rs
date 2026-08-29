use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{ItemTrait, Path, TraitItem, Type, TypeParamBound, WherePredicate};

use super::arguments::OwnerKind;
use super::decision::Decision;

pub fn assemble(
    domain_path: &Path,
    mut item: ItemTrait,
    decisions: &[Decision],
    owner_kind: OwnerKind,
) -> syn::Result<TokenStream> {
    add_supertraits(&mut item, owner_supertrait(domain_path, owner_kind));
    add_type_predicates(domain_path, &mut item, decisions)?;
    super::decision_reference::add(domain_path, &mut item, decisions)?;
    add_descriptors(domain_path, &mut item, decisions)?;
    add_attribute_requirement(domain_path, &mut item)?;
    Ok(quote!(#item))
}

fn owner_supertrait(domain_path: &Path, owner_kind: OwnerKind) -> TypeParamBound {
    match owner_kind {
        OwnerKind::Aggregate => {
            syn::parse_quote!(#domain_path::AggregateDecisionOwnerType)
        }
        OwnerKind::DomainService => {
            syn::parse_quote!(#domain_path::DomainServiceDecisionOwnerType)
        }
        OwnerKind::Entity => syn::parse_quote!(#domain_path::EntityDecisionOwnerType),
        OwnerKind::ValueObject => {
            syn::parse_quote!(#domain_path::ValueObjectDecisionOwnerType)
        }
    }
}

fn add_supertraits(item: &mut ItemTrait, owner_supertrait: TypeParamBound) {
    item.supertraits.push(owner_supertrait);
    item.supertraits.push(syn::parse_quote!(Sized));
}

fn add_type_predicates(
    domain_path: &Path,
    item: &mut ItemTrait,
    decisions: &[Decision],
) -> syn::Result<()> {
    for decision in decisions {
        add_type_predicate(
            item,
            &decision.input,
            &quote!(#domain_path::DecisionInputType),
        )?;
        add_type_predicate(
            item,
            &decision.output,
            &quote!(#domain_path::DecisionOutputType),
        )?;
    }
    Ok(())
}

fn add_type_predicate(item: &mut ItemTrait, ty: &Type, bound: &TokenStream) -> syn::Result<()> {
    let predicate: WherePredicate = syn::parse2(quote_spanned! {ty.span()=> #ty: #bound})?;
    item.generics.make_where_clause().predicates.push(predicate);
    Ok(())
}

fn add_descriptors(
    domain_path: &Path,
    item: &mut ItemTrait,
    decisions: &[Decision],
) -> syn::Result<()> {
    let descriptors = decisions
        .iter()
        .map(|decision| assemble_descriptor(domain_path, decision));
    let span = item.ident.span();
    let constant: TraitItem = syn::parse2(quote_spanned! {span=>
        #[doc(hidden)]
        const __DOMAIN_DECISIONS: &'static [#domain_path::DecisionDescriptor] = &[
            #(#descriptors),*
        ];
    })?;
    item.items.push(constant);
    Ok(())
}

fn assemble_descriptor(domain_path: &Path, decision: &Decision) -> TokenStream {
    let id = &decision.id;
    let label = &decision.label;
    let input = &decision.input;
    let output = &decision.output;

    quote! {
        #domain_path::DecisionDescriptor {
            id: #domain_path::DecisionId {
                owner: <Self as #domain_path::DecisionOwnerType>::DECISION_OWNER_ID,
                local: #id,
            },
            label: #label,
            input: <#input as #domain_path::DecisionInputType>::DESCRIPTOR,
            output: <#output as #domain_path::DecisionOutputType>::DESCRIPTOR,
            implementation: #domain_path::DecisionImplementationDescriptor::Rust,
        }
    }
}

fn add_attribute_requirement(domain_path: &Path, item: &mut ItemTrait) -> syn::Result<()> {
    let span = item.ident.span();
    let constant: TraitItem = syn::parse2(quote_spanned! {span=>
        #[doc(hidden)]
        const __DOMAIN_DECISIONS_TRAIT_REQUIRES_DOMAIN_DECISIONS_ATTRIBUTE: &'static [
            #domain_path::DecisionDescriptor
        ] = Self::__DOMAIN_DECISIONS;
    })?;
    item.items.push(constant);
    Ok(())
}
