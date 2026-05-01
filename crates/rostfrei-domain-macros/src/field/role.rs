use syn::{Attribute, PathArguments, Result, Token, TypePath, token};

use super::ir::Role;

pub fn parse(attributes: &[Attribute]) -> Result<Option<Role>> {
    let mut role = None;
    for attribute in attributes
        .iter()
        .filter(|attribute| crate::helper::domain_attribute::is_helper(attribute))
    {
        attribute.parse_nested_meta(|meta| {
            let has_arguments = meta.input.peek(Token![=]) || meta.input.peek(token::Paren);
            let parsed = if meta.path.is_ident("entity") && !has_arguments {
                Role::Entity
            } else if meta.path.is_ident("aggregate_ref") {
                let target = meta.value()?.parse::<TypePath>()?;
                if !is_direct_non_generic(&target) {
                    return Err(
                        meta.error("aggregate_ref target must be a direct, non-generic type path")
                    );
                }
                Role::AggregateReference(target)
            } else if meta.path.is_ident("scalar") {
                let provider = meta.value()?.parse::<TypePath>()?;
                if !is_direct_non_generic(&provider) {
                    return Err(
                        meta.error("scalar provider must be a direct, non-generic type path")
                    );
                }
                Role::SemanticScalar(provider)
            } else if meta.path.is_ident("entity") {
                return Err(meta.error("field role does not accept a value or arguments"));
            } else {
                return Err(meta.error("unsupported field domain attribute"));
            };
            if role.is_some() {
                return Err(meta.error("field supports at most one domain role"));
            }
            role = Some(parsed);
            Ok(())
        })?;
    }
    Ok(role)
}

fn is_direct_non_generic(path: &TypePath) -> bool {
    path.qself.is_none()
        && path
            .path
            .segments
            .iter()
            .all(|segment| matches!(segment.arguments, PathArguments::None))
}
