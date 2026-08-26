use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Attribute, ItemFn, Meta, MetaList, Token};

use super::DomainTestKind;

pub(crate) fn validate_function(function: &ItemFn, kind: DomainTestKind) -> syn::Result<()> {
    validate_signature(function, kind)?;
    validate_attributes(function, kind)
}

pub(crate) fn companion_attributes(function: &ItemFn) -> syn::Result<Vec<Attribute>> {
    let mut attributes = Vec::new();
    for attribute in &function.attrs {
        if attribute.path().is_ident("cfg") {
            attributes.push(attribute.clone());
        } else if attribute.path().is_ident("cfg_attr")
            && let Some(meta) = cfg_only_meta(&attribute.meta)?
        {
            let mut attribute = attribute.clone();
            attribute.meta = meta;
            attributes.push(attribute);
        }
    }
    Ok(attributes)
}

fn cfg_only_meta(meta: &Meta) -> syn::Result<Option<Meta>> {
    if meta.path().is_ident("cfg") {
        return Ok(Some(meta.clone()));
    }
    if !meta.path().is_ident("cfg_attr") {
        return Ok(None);
    }
    let Meta::List(list) = meta else {
        return Err(syn::Error::new_spanned(
            meta,
            "cfg_attr must contain a condition and attributes",
        ));
    };
    let entries = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
    let Some(condition) = entries.first() else {
        return Err(syn::Error::new_spanned(
            list,
            "cfg_attr must contain a condition and attributes",
        ));
    };
    let retained = entries
        .iter()
        .skip(1)
        .map(cfg_only_meta)
        .collect::<syn::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if retained.is_empty() {
        return Ok(None);
    }
    let tokens = quote!(#condition #(, #retained)*);
    Ok(Some(Meta::List(MetaList {
        path: list.path.clone(),
        delimiter: list.delimiter.clone(),
        tokens,
    })))
}

fn validate_signature(function: &ItemFn, kind: DomainTestKind) -> syn::Result<()> {
    let signature = &function.sig;
    if !signature.inputs.is_empty() {
        return Err(syn::Error::new_spanned(
            &signature.inputs,
            format!("{} tests cannot accept parameters", kind.name()),
        ));
    }
    if !signature.generics.params.is_empty() || signature.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &signature.generics,
            format!("{} tests cannot have generics", kind.name()),
        ));
    }
    if let Some(asyncness) = signature.asyncness {
        return Err(syn::Error::new_spanned(
            asyncness,
            format!("{} tests cannot be async", kind.name()),
        ));
    }
    if let Some(constness) = signature.constness {
        return Err(syn::Error::new_spanned(
            constness,
            format!("{} tests cannot be const", kind.name()),
        ));
    }
    if let Some(unsafety) = signature.unsafety {
        return Err(syn::Error::new_spanned(
            unsafety,
            format!("{} tests cannot be unsafe", kind.name()),
        ));
    }
    if let Some(abi) = &signature.abi {
        return Err(syn::Error::new_spanned(
            abi,
            format!("{} tests cannot have an extern ABI", kind.name()),
        ));
    }
    if let Some(variadic) = &signature.variadic {
        return Err(syn::Error::new_spanned(
            variadic,
            format!("{} tests cannot be variadic", kind.name()),
        ));
    }
    Ok(())
}

fn validate_attributes(function: &ItemFn, kind: DomainTestKind) -> syn::Result<()> {
    for attribute in &function.attrs {
        if attribute.path().is_ident("test") {
            return Err(syn::Error::new_spanned(
                attribute,
                format!(
                    "{} tests own the `#[test]` attribute; remove the authored `#[test]`",
                    kind.name()
                ),
            ));
        }
        if is_domain_test_attribute(attribute) {
            return Err(syn::Error::new_spanned(
                attribute,
                "domain test attributes cannot be stacked on the same function",
            ));
        }
    }
    Ok(())
}

fn is_domain_test_attribute(attribute: &Attribute) -> bool {
    let Some(name) = attribute
        .path()
        .segments
        .last()
        .map(|segment| &segment.ident)
    else {
        return false;
    };
    matches!(
        name.to_string().as_str(),
        "action_test"
            | "decision_test"
            | "invariant_test"
            | "lifecycle_test"
            | "domain_action_test"
            | "domain_decision_test"
            | "domain_invariant_test"
            | "domain_lifecycle_test"
            | "domain_test_action"
            | "domain_test_decision"
            | "domain_test_invariant"
            | "domain_test_lifecycle"
    )
}
