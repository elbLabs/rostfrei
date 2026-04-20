use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, PathArguments, Token, TypePath};

#[derive(Clone, Copy)]
pub enum OwnerKind {
    Aggregate,
    Entity,
}

pub struct Arguments {
    pub owner_kind: OwnerKind,
    pub group: TypePath,
}

impl Parse for Arguments {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Err(
                input.error("domain decision owner kind and `group = GroupType` are required")
            );
        }

        let mut owner_kind = None;
        let mut group = None;
        while !input.is_empty() {
            if input.peek(Token![pub]) {
                return Err(input.error(
                    "domain_decisions does not accept visibility syntax; declare the decision group as a normal Rust type with the desired visibility",
                ));
            }
            let key = Ident::parse_any(input)?;
            match key.to_string().as_str() {
                "aggregate" | "entity" => {
                    if input.peek(Token![=]) {
                        return Err(syn::Error::new(
                            key.span(),
                            "domain decision owner kinds must be unkeyed; use `aggregate` or `entity`",
                        ));
                    }
                    if owner_kind.is_some() {
                        return Err(syn::Error::new(
                            key.span(),
                            "duplicate domain decision owner kind",
                        ));
                    }
                    owner_kind = Some(if key == "aggregate" {
                        OwnerKind::Aggregate
                    } else {
                        OwnerKind::Entity
                    });
                }
                "group" => {
                    if group.is_some() {
                        return Err(syn::Error::new(key.span(), "duplicate group"));
                    }
                    input.parse::<Token![=]>().map_err(|_| {
                        syn::Error::new(key.span(), "decision group requires `group = GroupType`")
                    })?;
                    if input.peek(Token![pub]) {
                        return Err(input.error(
                            "domain_decisions does not accept group visibility; declare the referenced group type with normal Rust visibility",
                        ));
                    }
                    let path = input.parse::<TypePath>()?;
                    validate_group_path(&path)?;
                    group = Some(path);
                }
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!(
                            "unsupported domain_decisions argument `{key}`; expected `aggregate` or `entity`, and `group = GroupType`"
                        ),
                    ));
                }
            }

            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }

        Ok(Self {
            owner_kind: owner_kind.ok_or_else(|| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "domain decision owner kind is required; expected `aggregate` or `entity`",
                )
            })?,
            group: group.ok_or_else(|| {
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "domain decision group is required; use `group = GroupType`",
                )
            })?,
        })
    }
}

fn validate_group_path(group: &TypePath) -> syn::Result<()> {
    if group.qself.is_some() {
        return Err(syn::Error::new_spanned(
            group,
            "decision group must be a normal, non-generic type path without qualified self syntax",
        ));
    }
    if let Some(arguments) =
        group
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
    Ok(())
}

pub fn parse(tokens: proc_macro2::TokenStream) -> syn::Result<Arguments> {
    syn::parse2(tokens)
}
