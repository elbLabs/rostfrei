use std::collections::HashSet;

use quote::ToTokens;
use syn::parse::{Parse, ParseStream};
use syn::{Path, PathArguments, Result, Token, TypePath, bracketed};

pub fn parse(input: ParseStream) -> Result<Vec<Path>> {
    let content;
    bracketed!(content in input);
    let entries = content.parse_terminated(TypePath::parse, Token![,])?;
    let mut lexical_paths = HashSet::new();
    let mut groups = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.qself.is_some() {
            return Err(syn::Error::new_spanned(
                entry,
                "decision group paths cannot use qualified self syntax",
            ));
        }
        if let Some(arguments) =
            entry
                .path
                .segments
                .iter()
                .find_map(|segment| match &segment.arguments {
                    PathArguments::None => None,
                    arguments => Some(arguments),
                })
        {
            return Err(syn::Error::new_spanned(
                arguments,
                "decision group paths cannot contain generic arguments",
            ));
        }
        let lexical_path = entry.path.to_token_stream().to_string();
        if !lexical_paths.insert(lexical_path) {
            return Err(syn::Error::new_spanned(
                &entry.path,
                "duplicate decision group path",
            ));
        }
        groups.push(entry.path);
    }
    Ok(groups)
}
