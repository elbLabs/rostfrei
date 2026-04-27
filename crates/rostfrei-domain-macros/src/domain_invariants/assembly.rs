use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{ItemTrait, Path, TraitItem};

use super::invariant::Invariant;

pub fn assemble(
    domain_path: &Path,
    mut item: ItemTrait,
    invariants: &[Invariant],
) -> syn::Result<TokenStream> {
    super::invariant_reference::add(domain_path, &mut item, invariants)?;
    add_descriptors(domain_path, &mut item, invariants)?;
    Ok(quote!(#item))
}

fn add_descriptors(
    domain_path: &Path,
    item: &mut ItemTrait,
    invariants: &[Invariant],
) -> syn::Result<()> {
    let descriptors = invariants.iter().map(|invariant| {
        let id = &invariant.id;
        let label = &invariant.label;
        quote! {
            #domain_path::InvariantDescriptor {
                id: #domain_path::InvariantId(#id),
                label: #label,
            }
        }
    });
    let span = item.ident.span();
    let constant: TraitItem = syn::parse2(quote_spanned! {span=>
        #[doc(hidden)]
        const __DOMAIN_INVARIANTS: &'static [#domain_path::InvariantDescriptor] = &[
            #(#descriptors),*
        ];
    })?;
    item.items.push(constant);
    Ok(())
}
