use proc_macro2::{Ident, TokenStream};
use quote::{ToTokens, quote_spanned};
use syn::ext::IdentExt;
use syn::{Path, PathArguments, parse::ParseStream};

use super::ir::ActionReferencePath;

pub fn parse(input: ParseStream) -> syn::Result<ActionReferencePath> {
    let mut path: Path = input.parse()?;
    validate_path(&path)?;
    let Some(reference_segment) = path.segments.pop() else {
        return Err(syn::Error::new_spanned(
            &path,
            "transition action must use `TraitPath::REFERENCE`",
        ));
    };
    let reference_segment = reference_segment.into_value();
    path.segments.pop_punct();
    let reference = reference_segment.ident;
    validate_reference(&reference)?;
    let lexical = format!("{}::{}", path.to_token_stream(), reference.unraw());
    let span = reference.span();
    Ok(ActionReferencePath {
        trait_path: path,
        reference,
        span,
        lexical,
    })
}

pub fn assemble_id(
    domain_path: &Path,
    reference: &ActionReferencePath,
    owner: &syn::TypePath,
) -> TokenStream {
    let trait_path = &reference.trait_path;
    let hidden = crate::helper::action_reference::hidden_from_public(&reference.reference);
    let span = reference.span;
    quote_spanned! {span=>
        {
            let _: &'static [#domain_path::ActionDescriptor] =
                <#owner as #trait_path>::__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE;
            let reference: #domain_path::ActionReference<#owner> =
                <#owner as #trait_path>::#hidden;
            reference.id()
        }
    }
}

fn validate_path(path: &Path) -> syn::Result<()> {
    if path.segments.len() < 2 {
        return Err(syn::Error::new_spanned(
            path,
            "transition action must use `TraitPath::REFERENCE`",
        ));
    }
    if let Some(arguments) = path
        .segments
        .iter()
        .find_map(|segment| match &segment.arguments {
            PathArguments::None => None,
            arguments => Some(arguments),
        })
    {
        return Err(syn::Error::new_spanned(
            arguments,
            "transition action paths cannot contain generic arguments",
        ));
    }
    Ok(())
}

fn validate_reference(reference: &Ident) -> syn::Result<()> {
    if crate::helper::action_reference::is_hidden(reference) {
        return Err(syn::Error::new(
            reference.span(),
            "transition actions must use the public reference name without the generated prefix",
        ));
    }
    if !crate::helper::action_reference::is_canonical_public(reference) {
        return Err(syn::Error::new(
            reference.span(),
            "transition action references must use canonical uppercase names such as `ACTIVATE` or `_2FA_START`",
        ));
    }
    Ok(())
}
