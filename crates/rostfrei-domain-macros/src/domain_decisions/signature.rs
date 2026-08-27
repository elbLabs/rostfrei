use syn::{FnArg, GenericArgument, Pat, PathArguments, ReturnType, Signature, Type, TypePath};

use super::decision::Parameter;

pub struct DecisionTypes {
    pub parameters: Vec<Parameter>,
    pub output: Type,
    pub error: Type,
}

pub fn parse(signature: &Signature) -> syn::Result<DecisionTypes> {
    validate_qualifiers(signature)?;
    let parameters = parse_parameters(signature)?;
    let (output, error) = parse_output(signature)?;
    Ok(DecisionTypes {
        parameters,
        output,
        error,
    })
}

fn validate_qualifiers(signature: &Signature) -> syn::Result<()> {
    if signature.variadic.is_some()
        || signature.asyncness.is_some()
        || signature.unsafety.is_some()
        || signature.abi.is_some()
        || !signature.generics.params.is_empty()
        || signature.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(
            signature,
            "decisions cannot be async, unsafe, extern, variadic, generic, or have where clauses",
        ));
    }
    Ok(())
}

fn parse_parameters(signature: &Signature) -> syn::Result<Vec<Parameter>> {
    if let Some(receiver) = signature.inputs.iter().find_map(|input| match input {
        FnArg::Receiver(receiver) => Some(receiver),
        FnArg::Typed(_) => None,
    }) {
        return Err(syn::Error::new_spanned(
            receiver,
            "decisions must be associated functions without a receiver",
        ));
    }
    signature.inputs.iter().map(parse_parameter).collect()
}

fn parse_parameter(input: &FnArg) -> syn::Result<Parameter> {
    let FnArg::Typed(input) = input else {
        return Err(syn::Error::new_spanned(input, "unexpected receiver"));
    };
    let name = validate_parameter_pattern(&input.pat)?;
    validate_owned_type(&input.ty)?;
    Ok(Parameter {
        name,
        ty: (*input.ty).clone(),
    })
}

fn validate_parameter_pattern(pattern: &Pat) -> syn::Result<syn::Ident> {
    let Pat::Ident(pattern) = pattern else {
        return Err(invalid_parameter_pattern(pattern));
    };
    if !pattern.attrs.is_empty()
        || pattern.by_ref.is_some()
        || pattern.mutability.is_some()
        || pattern.subpat.is_some()
    {
        return Err(invalid_parameter_pattern(pattern));
    }
    Ok(pattern.ident.clone())
}

fn invalid_parameter_pattern(pattern: impl quote::ToTokens) -> syn::Error {
    syn::Error::new_spanned(
        pattern,
        "decision parameters must use simple, immutable identifiers",
    )
}

fn validate_owned_type(ty: &Type) -> syn::Result<()> {
    match ty {
        Type::Group(group) => validate_owned_type(&group.elem),
        Type::Paren(paren) => validate_owned_type(&paren.elem),
        Type::Reference(_) => Err(syn::Error::new_spanned(
            ty,
            "decision parameters must use owned types, not references",
        )),
        _ => Ok(()),
    }
}

fn parse_output(signature: &Signature) -> syn::Result<(Type, Type)> {
    let output = match &signature.output {
        ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &signature.ident,
                "decisions must return Result<T, E>",
            ));
        }
        ReturnType::Type(_, output) => output.as_ref(),
    };
    split_result(output)
        .ok_or_else(|| syn::Error::new_spanned(output, "decisions must return Result<T, E>"))
}

fn split_result(output: &Type) -> Option<(Type, Type)> {
    let Type::Path(TypePath { qself: None, path }) = output else {
        return None;
    };
    let names: Vec<_> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    if names.as_slice() != ["Result"]
        && names.as_slice() != ["core", "result", "Result"]
        && names.as_slice() != ["std", "result", "Result"]
    {
        return None;
    }
    if path
        .segments
        .iter()
        .take(path.segments.len().saturating_sub(1))
        .any(|segment| !matches!(segment.arguments, PathArguments::None))
    {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &path.segments.last()?.arguments else {
        return None;
    };
    let mut arguments = arguments.args.iter();
    let GenericArgument::Type(output) = arguments.next()? else {
        return None;
    };
    let GenericArgument::Type(error) = arguments.next()? else {
        return None;
    };
    arguments
        .next()
        .is_none()
        .then(|| (output.clone(), error.clone()))
}
