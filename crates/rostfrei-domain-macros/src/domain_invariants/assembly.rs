use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{ItemTrait, TraitItem, TypeParamBound};

use super::arguments::OwnerKind;
use super::invariant::Invariant;

pub fn assemble(
    mut item: ItemTrait,
    invariants: &[Invariant],
    owner_kind: OwnerKind,
) -> syn::Result<TokenStream> {
    add_supertraits(&mut item, owner_supertrait(owner_kind));
    super::invariant_reference::add(&mut item, invariants)?;
    add_descriptors(&mut item, invariants)?;
    add_attribute_requirement(&mut item)?;
    add_append_violations(&mut item, invariants)?;
    Ok(quote!(#item))
}

fn owner_supertrait(owner_kind: OwnerKind) -> TypeParamBound {
    match owner_kind {
        OwnerKind::Aggregate => {
            syn::parse_quote!(::domain::AggregateInvariantOwnerType)
        }
        OwnerKind::Entity => syn::parse_quote!(::domain::EntityInvariantOwnerType),
        OwnerKind::ValueObject => {
            syn::parse_quote!(::domain::ValueObjectInvariantOwnerType)
        }
    }
}

fn add_supertraits(item: &mut ItemTrait, owner_supertrait: TypeParamBound) {
    item.supertraits.push(owner_supertrait);
    item.supertraits
        .push(syn::parse_quote!(::core::marker::Sized));
}

fn add_descriptors(item: &mut ItemTrait, invariants: &[Invariant]) -> syn::Result<()> {
    let descriptors = invariants.iter().map(assemble_descriptor);
    let span = item.ident.span();
    let constant: TraitItem = syn::parse2(quote_spanned! {span=>
        #[doc(hidden)]
        const __DOMAIN_INVARIANTS: &'static [::domain::InvariantDescriptor] = &[
            #(#descriptors),*
        ];
    })?;
    item.items.push(constant);
    Ok(())
}

fn assemble_descriptor(invariant: &Invariant) -> TokenStream {
    let id = &invariant.id;
    let label = &invariant.label;

    quote! {
        ::domain::InvariantDescriptor {
            id: ::domain::InvariantId {
                owner: <Self as ::domain::InvariantOwnerType>::INVARIANT_OWNER_ID,
                local: #id,
            },
            label: #label,
        }
    }
}

fn add_attribute_requirement(item: &mut ItemTrait) -> syn::Result<()> {
    let span = item.ident.span();
    let constant: TraitItem = syn::parse2(quote_spanned! {span=>
        #[doc(hidden)]
        const __DOMAIN_INVARIANTS_TRAIT_REQUIRES_DOMAIN_INVARIANTS_ATTRIBUTE: &'static [
            ::domain::InvariantDescriptor
        ] = Self::__DOMAIN_INVARIANTS;
    })?;
    item.items.push(constant);
    Ok(())
}

fn add_append_violations(item: &mut ItemTrait, invariants: &[Invariant]) -> syn::Result<()> {
    let trait_name = &item.ident;
    let checks = invariants.iter().map(|invariant| {
        let method = &invariant.method;
        quote! {
            if let ::core::option::Option::Some(violation) =
                <Self as #trait_name>::#method(candidate)
            {
                violations.push(violation);
            }
        }
    });
    let span = item.ident.span();
    let method: TraitItem = syn::parse2(quote_spanned! {span=>
        #[doc(hidden)]
        fn __DOMAIN_INVARIANTS_APPEND_VIOLATIONS(
            candidate: &<Self as ::domain::InvariantOwnerType>::Candidate,
            violations: &mut ::std::vec::Vec<::domain::InvariantViolation>,
        ) {
            #(#checks)*
        }
    })?;
    item.items.push(method);
    Ok(())
}
