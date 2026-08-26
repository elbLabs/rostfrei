use std::collections::HashSet;

use super::attributes::Query;

pub fn validate(queries: &mut [Query]) -> syn::Result<()> {
    if queries.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "domain_queries requires at least one query",
        ));
    }
    let mut ids = HashSet::new();
    let mut errors = None;
    for query in queries.iter_mut() {
        if let Err(error) = crate::helper::id::validate(&query.id) {
            combine(&mut errors, error);
        }
        if let Err(error) = crate::helper::label::validate(&query.label) {
            combine(&mut errors, error);
        }
        if !ids.insert(query.id.value()) {
            combine(
                &mut errors,
                syn::Error::new(query.id.span(), "duplicate query id"),
            );
        }
        if let Err(error) = validate_qualifiers(&query.syntax) {
            combine(&mut errors, error);
        } else {
            match super::signature::parse(&query.syntax, &query.visibility) {
                Ok(signature) => query.signature = Some(signature),
                Err(error) => combine(&mut errors, error),
            }
        }
    }
    errors.map_or(Ok(()), Err)
}

fn combine(errors: &mut Option<syn::Error>, error: syn::Error) {
    match errors {
        Some(errors) => errors.combine(error),
        None => *errors = Some(error),
    }
}

fn validate_qualifiers(signature: &syn::Signature) -> syn::Result<()> {
    if signature.variadic.is_some()
        || signature.asyncness.is_some()
        || signature.unsafety.is_some()
        || signature.abi.is_some()
        || !signature.generics.params.is_empty()
        || signature.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(
            signature,
            "queries cannot be async, unsafe, extern, variadic, generic, or have where clauses",
        ));
    }
    Ok(())
}
