use std::collections::HashSet;

use quote::ToTokens as _;
use syn::{Data, DeriveInput, Fields, Type, TypePath};

pub struct EventVariant {
    pub name: syn::Ident,
    pub event: TypePath,
}

pub fn extract(input: &DeriveInput) -> syn::Result<Vec<EventVariant>> {
    validate_generics(input)?;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "AggregateEvents only supports non-generic enums",
        ));
    };
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "AggregateEvents requires at least one event variant",
        ));
    }

    let mut event_types = HashSet::new();
    data.variants
        .iter()
        .map(|variant| {
            let Fields::Unnamed(fields) = &variant.fields else {
                return Err(syn::Error::new_spanned(
                    variant,
                    "AggregateEvents variants must contain exactly one unnamed event field",
                ));
            };
            if fields.unnamed.len() != 1 {
                return Err(syn::Error::new_spanned(
                    fields,
                    "AggregateEvents variants must contain exactly one unnamed event field",
                ));
            }
            let Some(field) = fields.unnamed.first() else {
                return Err(syn::Error::new_spanned(
                    fields,
                    "AggregateEvents variants must contain exactly one unnamed event field",
                ));
            };
            let Type::Path(event) = &field.ty else {
                return Err(syn::Error::new_spanned(
                    &field.ty,
                    "AggregateEvents event fields must use concrete type paths",
                ));
            };
            if event.qself.is_some()
                || event
                    .path
                    .segments
                    .iter()
                    .any(|segment| !matches!(segment.arguments, syn::PathArguments::None))
            {
                return Err(syn::Error::new_spanned(
                    event,
                    "AggregateEvents event fields must use direct, non-generic type paths",
                ));
            }
            let key = event.to_token_stream().to_string();
            if !event_types.insert(key) {
                return Err(syn::Error::new_spanned(
                    event,
                    "AggregateEvents cannot contain the same event type more than once",
                ));
            }
            Ok(EventVariant {
                name: variant.ident.clone(),
                event: event.clone(),
            })
        })
        .collect()
}

fn validate_generics(input: &DeriveInput) -> syn::Result<()> {
    if input.generics.params.is_empty() && input.generics.where_clause.is_none() {
        return Ok(());
    }
    Err(syn::Error::new_spanned(
        &input.generics,
        "AggregateEvents only supports non-generic enums",
    ))
}
