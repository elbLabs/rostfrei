use proc_macro2::TokenStream;
use quote::quote;
use syn::{ItemTrait, Path, TraitItem};

use super::attributes::Attributes;

pub fn assemble(domain_path: &Path, mut item: ItemTrait, attributes: &Attributes) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    item.items.extend([
        TraitItem::Verbatim(quote! {
            const LOCAL_ID: &'static str = #id;
        }),
        TraitItem::Verbatim(quote! {
            const LABEL: &'static str = #label;
        }),
        TraitItem::Verbatim(quote! {
            const DESCRIPTOR: #domain_path::InvariantDescriptor =
                #domain_path::InvariantDescriptor {
                    id: #domain_path::InvariantId(#id),
                    label: #label,
                };
        }),
    ]);
    quote!(#item)
}
