use proc_macro2::TokenStream;
use quote::quote;
use syn::{ItemTrait, Path, TraitItem};

use super::attributes::Attributes;

pub fn assemble(domain_path: &Path, mut item: ItemTrait, attributes: &Attributes) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let local_id = TraitItem::Verbatim(quote! {
        const LOCAL_ID: &'static str = #id;
    });
    let label_item = TraitItem::Verbatim(quote! {
        const LABEL: &'static str = #label;
    });
    let descriptor = TraitItem::Verbatim(quote! {
        const DESCRIPTOR: #domain_path::ActionDescriptor = #domain_path::ActionDescriptor {
            id: #domain_path::ActionId(#id),
            label: #label,
        };
    });
    item.items.extend([local_id, label_item, descriptor]);
    quote!(#item)
}
