use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{ItemTrait, TraitItem, Type, TypeParamBound, WherePredicate};

use super::arguments::OwnerKind;
use super::decision::Decision;

pub fn assemble(
    mut item: ItemTrait,
    decisions: &[Decision],
    owner_kind: OwnerKind,
) -> syn::Result<TokenStream> {
    add_supertraits(&mut item, owner_supertrait(owner_kind));
    add_type_predicates(&mut item, decisions)?;
    super::decision_reference::add(&mut item, decisions)?;
    add_descriptors(&mut item, decisions)?;
    add_attribute_requirement(&mut item)?;
    Ok(quote!(#item))
}

fn owner_supertrait(owner_kind: OwnerKind) -> TypeParamBound {
    match owner_kind {
        OwnerKind::Aggregate => {
            syn::parse_quote!(::domain::AggregateDecisionOwnerType)
        }
        OwnerKind::DomainService => {
            syn::parse_quote!(::domain::DomainServiceDecisionOwnerType)
        }
        OwnerKind::Entity => syn::parse_quote!(::domain::EntityDecisionOwnerType),
        OwnerKind::ValueObject => {
            syn::parse_quote!(::domain::ValueObjectDecisionOwnerType)
        }
    }
}

fn add_supertraits(item: &mut ItemTrait, owner_supertrait: TypeParamBound) {
    item.supertraits.push(owner_supertrait);
    item.supertraits.push(syn::parse_quote!(Sized));
}

fn add_type_predicates(item: &mut ItemTrait, decisions: &[Decision]) -> syn::Result<()> {
    for decision in decisions {
        add_type_predicate(item, &decision.input, quote!(::domain::DecisionInputType))?;
        add_type_predicate(item, &decision.output, quote!(::domain::DecisionOutputType))?;
    }
    Ok(())
}

fn add_type_predicate(item: &mut ItemTrait, ty: &Type, bound: TokenStream) -> syn::Result<()> {
    let predicate: WherePredicate = syn::parse2(quote_spanned! {ty.span()=> #ty: #bound})?;
    item.generics.make_where_clause().predicates.push(predicate);
    Ok(())
}

fn add_descriptors(item: &mut ItemTrait, decisions: &[Decision]) -> syn::Result<()> {
    let descriptors = decisions.iter().map(assemble_descriptor);
    let span = item.ident.span();
    let constant: TraitItem = syn::parse2(quote_spanned! {span=>
        #[doc(hidden)]
        const __DOMAIN_DECISIONS: &'static [::domain::DecisionDescriptor] = &[
            #(#descriptors),*
        ];
    })?;
    item.items.push(constant);
    Ok(())
}

fn assemble_descriptor(decision: &Decision) -> TokenStream {
    let id = &decision.id;
    let label = &decision.label;
    let input = &decision.input;
    let output = &decision.output;

    quote! {
        ::domain::DecisionDescriptor {
            id: ::domain::DecisionId {
                owner: <Self as ::domain::DecisionOwnerType>::DECISION_OWNER_ID,
                local: #id,
            },
            label: #label,
            input: <#input as ::domain::DecisionInputType>::DESCRIPTOR,
            output: <#output as ::domain::DecisionOutputType>::DESCRIPTOR,
            implementation: ::domain::DecisionImplementationDescriptor::Rust,
        }
    }
}

fn add_attribute_requirement(item: &mut ItemTrait) -> syn::Result<()> {
    let span = item.ident.span();
    let constant: TraitItem = syn::parse2(quote_spanned! {span=>
        #[doc(hidden)]
        const __DOMAIN_DECISIONS_TRAIT_REQUIRES_DOMAIN_DECISIONS_ATTRIBUTE: &'static [
            ::domain::DecisionDescriptor
        ] = Self::__DOMAIN_DECISIONS;
    })?;
    item.items.push(constant);
    Ok(())
}
