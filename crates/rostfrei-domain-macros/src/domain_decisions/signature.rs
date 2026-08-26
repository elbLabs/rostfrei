use syn::{FnArg, Pat, ReturnType, Signature, Type};

pub struct DecisionTypes {
    pub input: Type,
    pub output: Type,
}

pub fn parse(signature: &Signature) -> syn::Result<DecisionTypes> {
    validate_qualifiers(signature)?;
    validate_generics(signature)?;

    Ok(DecisionTypes {
        input: parse_input(signature)?,
        output: parse_output(signature)?,
    })
}

fn validate_qualifiers(signature: &Signature) -> syn::Result<()> {
    if let Some(asyncness) = &signature.asyncness {
        return Err(syn::Error::new_spanned(
            asyncness,
            "decisions cannot be async",
        ));
    }
    if let Some(unsafety) = &signature.unsafety {
        return Err(syn::Error::new_spanned(
            unsafety,
            "decisions cannot be unsafe",
        ));
    }
    if let Some(abi) = &signature.abi {
        return Err(syn::Error::new_spanned(abi, "decisions cannot be extern"));
    }
    if let Some(variadic) = &signature.variadic {
        return Err(syn::Error::new_spanned(
            variadic,
            "decisions cannot be variadic",
        ));
    }
    Ok(())
}

fn validate_generics(signature: &Signature) -> syn::Result<()> {
    if !signature.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &signature.generics.params,
            "decisions cannot have generic parameters",
        ));
    }
    if let Some(where_clause) = &signature.generics.where_clause {
        return Err(syn::Error::new_spanned(
            where_clause,
            "decisions cannot have method-level where clauses",
        ));
    }
    Ok(())
}

fn parse_input(signature: &Signature) -> syn::Result<Type> {
    if let Some(receiver) = signature.inputs.iter().find_map(|input| match input {
        FnArg::Receiver(receiver) => Some(receiver),
        FnArg::Typed(_) => None,
    }) {
        return Err(syn::Error::new_spanned(
            receiver,
            "domain decision contract methods must be associated functions without a receiver",
        ));
    }
    if signature.inputs.len() != 1 {
        return Err(syn::Error::new_spanned(
            &signature.inputs,
            "domain decision contract methods require exactly one owned input parameter",
        ));
    }

    let FnArg::Typed(input) = signature.inputs.first().unwrap() else {
        unreachable!()
    };
    validate_input_pattern(&input.pat)?;
    validate_owned_type(&input.ty)?;
    Ok((*input.ty).clone())
}

fn validate_input_pattern(pattern: &Pat) -> syn::Result<()> {
    let Pat::Ident(pattern) = pattern else {
        return Err(invalid_input_pattern(pattern));
    };
    if !pattern.attrs.is_empty()
        || pattern.by_ref.is_some()
        || pattern.mutability.is_some()
        || pattern.subpat.is_some()
        || pattern.ident != "input"
    {
        return Err(invalid_input_pattern(pattern));
    }
    Ok(())
}

fn invalid_input_pattern(pattern: impl quote::ToTokens) -> syn::Error {
    syn::Error::new_spanned(
        pattern,
        "decision parameter must be a simple identifier named `input`",
    )
}

fn validate_owned_type(ty: &Type) -> syn::Result<()> {
    match ty {
        Type::Group(group) => validate_owned_type(&group.elem),
        Type::Paren(paren) => validate_owned_type(&paren.elem),
        Type::Reference(_) => Err(syn::Error::new_spanned(
            ty,
            "decision input parameter must use an owned type, not a reference",
        )),
        _ => Ok(()),
    }
}

fn parse_output(signature: &Signature) -> syn::Result<Type> {
    match &signature.output {
        ReturnType::Default => Err(syn::Error::new_spanned(
            &signature.ident,
            "domain decision contract methods require an explicit output type",
        )),
        ReturnType::Type(_, output) => Ok((**output).clone()),
    }
}
