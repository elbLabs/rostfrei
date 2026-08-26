use syn::{FnArg, GenericArgument, Pat, Path, PathArguments, ReturnType, Signature, Type};

pub fn validate(signature: &Signature) -> syn::Result<()> {
    validate_qualifiers(signature)?;
    validate_generics(signature)?;
    validate_candidate(signature)?;
    validate_output(signature)
}

fn validate_qualifiers(signature: &Signature) -> syn::Result<()> {
    if let Some(asyncness) = &signature.asyncness {
        return Err(syn::Error::new_spanned(
            asyncness,
            "invariant checkers cannot be async",
        ));
    }
    if let Some(unsafety) = &signature.unsafety {
        return Err(syn::Error::new_spanned(
            unsafety,
            "invariant checkers cannot be unsafe",
        ));
    }
    if let Some(abi) = &signature.abi {
        return Err(syn::Error::new_spanned(
            abi,
            "invariant checkers cannot be extern",
        ));
    }
    if let Some(variadic) = &signature.variadic {
        return Err(syn::Error::new_spanned(
            variadic,
            "invariant checkers cannot be variadic",
        ));
    }
    Ok(())
}

fn validate_generics(signature: &Signature) -> syn::Result<()> {
    if !signature.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &signature.generics.params,
            "invariant checkers cannot have generic parameters",
        ));
    }
    if let Some(where_clause) = &signature.generics.where_clause {
        return Err(syn::Error::new_spanned(
            where_clause,
            "invariant checkers cannot have method-level where clauses",
        ));
    }
    Ok(())
}

fn validate_candidate(signature: &Signature) -> syn::Result<()> {
    if let Some(receiver) = signature.inputs.iter().find_map(|input| match input {
        FnArg::Receiver(receiver) => Some(receiver),
        FnArg::Typed(_) => None,
    }) {
        return Err(syn::Error::new_spanned(
            receiver,
            "domain invariant checker methods must be associated functions without a receiver",
        ));
    }
    if signature.inputs.len() != 1 {
        return Err(syn::Error::new_spanned(
            &signature.inputs,
            "domain invariant checker methods require exactly one candidate parameter",
        ));
    }

    let FnArg::Typed(candidate) = signature.inputs.first().unwrap() else {
        unreachable!()
    };
    validate_candidate_pattern(&candidate.pat)?;
    validate_candidate_type(&candidate.ty)
}

fn validate_candidate_pattern(pattern: &Pat) -> syn::Result<()> {
    let Pat::Ident(pattern) = pattern else {
        return Err(invalid_candidate_pattern(pattern));
    };
    if !pattern.attrs.is_empty()
        || pattern.by_ref.is_some()
        || pattern.mutability.is_some()
        || pattern.subpat.is_some()
        || pattern.ident != "candidate"
    {
        return Err(invalid_candidate_pattern(pattern));
    }
    Ok(())
}

fn invalid_candidate_pattern(pattern: impl quote::ToTokens) -> syn::Error {
    syn::Error::new_spanned(
        pattern,
        "invariant parameter must be a simple immutable identifier named `candidate`",
    )
}

fn validate_candidate_type(ty: &Type) -> syn::Result<()> {
    let Type::Reference(reference) = ty else {
        return Err(invalid_candidate_type(ty));
    };
    if reference.mutability.is_some()
        || reference.lifetime.is_some()
        || !is_invariant_owner_candidate(&reference.elem)
    {
        return Err(invalid_candidate_type(ty));
    }
    Ok(())
}

fn invalid_candidate_type(ty: impl quote::ToTokens) -> syn::Error {
    syn::Error::new_spanned(
        ty,
        "invariant candidate must be an immutable reference to `<Self as InvariantOwnerType>::Candidate` without an explicit lifetime",
    )
}

fn is_invariant_owner_candidate(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    let Some(qself) = &type_path.qself else {
        return false;
    };
    if !is_self_type(&qself.ty) {
        return false;
    }

    let segments: Vec<_> = type_path.path.segments.iter().collect();
    if segments.len() < 2 || qself.position + 1 != segments.len() {
        return false;
    }
    let candidate = segments[segments.len() - 1];
    if candidate.ident != "Candidate" || !matches!(candidate.arguments, PathArguments::None) {
        return false;
    }
    if segments[..qself.position]
        .iter()
        .any(|segment| !matches!(segment.arguments, PathArguments::None))
    {
        return false;
    }

    let owner_path: Vec<_> = segments[..qself.position]
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    owner_path.as_slice() == ["InvariantOwnerType"]
        || owner_path.as_slice() == ["rostfrei_domain", "InvariantOwnerType"]
}

fn is_self_type(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    type_path.qself.is_none()
        && type_path.path.leading_colon.is_none()
        && type_path.path.segments.len() == 1
        && type_path.path.segments[0].ident == "Self"
        && matches!(type_path.path.segments[0].arguments, PathArguments::None)
}

fn validate_output(signature: &Signature) -> syn::Result<()> {
    let ReturnType::Type(_, output) = &signature.output else {
        return Err(invalid_output(&signature.ident));
    };
    if !is_violation_option(output) {
        return Err(invalid_output(output));
    }
    Ok(())
}

fn invalid_output(tokens: impl quote::ToTokens) -> syn::Error {
    syn::Error::new_spanned(
        tokens,
        "invariant checkers must return exactly `Option<InvariantViolation>`",
    )
}

fn is_violation_option(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() || !is_option_path(&type_path.path) {
        return false;
    }

    let Some(last) = type_path.path.segments.last() else {
        return false;
    };
    let PathArguments::AngleBracketed(arguments) = &last.arguments else {
        return false;
    };
    if arguments.args.len() != 1 {
        return false;
    }
    let Some(GenericArgument::Type(violation)) = arguments.args.first() else {
        return false;
    };
    is_invariant_violation(violation)
}

fn is_option_path(path: &Path) -> bool {
    path_has_names(path, true, &["Option"])
        || path_has_names(path, true, &["core", "option", "Option"])
        || path_has_names(path, true, &["std", "option", "Option"])
}

fn is_invariant_violation(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() {
        return false;
    }
    path_has_names(&type_path.path, false, &["InvariantViolation"])
        || path_has_names(
            &type_path.path,
            false,
            &["rostfrei_domain", "InvariantViolation"],
        )
}

fn path_has_names(path: &Path, allow_last_arguments: bool, expected: &[&str]) -> bool {
    if path.segments.len() != expected.len() {
        return false;
    }
    let last = path.segments.len() - 1;
    path.segments
        .iter()
        .zip(expected)
        .enumerate()
        .all(|(position, (segment, expected))| {
            (position == last && allow_last_arguments
                || matches!(segment.arguments, PathArguments::None))
                && segment.ident == expected
        })
}
