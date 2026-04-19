use syn::spanned::Spanned;
use syn::{FnArg, GenericArgument, Pat, PathArguments, ReturnType, Signature, Type, TypePath};

pub struct ParsedSignature {
    pub root: Option<Type>,
    pub input: Option<Type>,
    pub output: Type,
    pub error: Option<Type>,
}

pub fn parse_entity(signature: &Signature) -> syn::Result<ParsedSignature> {
    let Some(FnArg::Receiver(receiver)) = signature.inputs.first() else {
        return Err(syn::Error::new_spanned(
            &signature.ident,
            "domain action contract methods require an &self or &mut self receiver",
        ));
    };
    if receiver.colon_token.is_some() {
        return Err(syn::Error::new_spanned(
            receiver,
            "domain action contract methods do not support typed receivers",
        ));
    }
    let Some((_, lifetime)) = &receiver.reference else {
        return Err(syn::Error::new_spanned(
            receiver,
            "domain action contract methods require an &self or &mut self receiver",
        ));
    };
    if let Some(lifetime) = lifetime {
        return Err(syn::Error::new_spanned(
            lifetime,
            "domain action contract receivers cannot have an explicit lifetime",
        ));
    }
    let input = parse_business_inputs(signature.inputs.iter().skip(1), 1, "entity")?;
    Ok(parsed(None, input, signature))
}

pub fn parse_domain_service(signature: &Signature) -> syn::Result<ParsedSignature> {
    if let Some(FnArg::Receiver(receiver)) = signature.inputs.first() {
        return Err(syn::Error::new_spanned(
            receiver,
            "domain service action contract methods must be associated functions without a receiver",
        ));
    }
    let input = parse_business_inputs(signature.inputs.iter(), 1, "domain service")?;
    Ok(parsed(None, input, signature))
}

pub fn parse_value_object(signature: &Signature) -> syn::Result<ParsedSignature> {
    let input = match signature.inputs.first() {
        Some(FnArg::Receiver(receiver)) if receiver.colon_token.is_some() => {
            return Err(syn::Error::new_spanned(
                receiver,
                "value object action contract methods do not support typed receivers",
            ));
        }
        Some(FnArg::Receiver(receiver)) if receiver.reference.is_some() => {
            return Err(syn::Error::new_spanned(
                receiver,
                "value object transformations require a consuming self receiver",
            ));
        }
        Some(FnArg::Receiver(_)) => parse_business_inputs(
            signature.inputs.iter().skip(1),
            1,
            "value object transformation",
        )?,
        _ => {
            let input =
                parse_business_inputs(signature.inputs.iter(), 1, "value object constructor")?;
            if input.is_none() {
                return Err(syn::Error::new_spanned(
                    &signature.inputs,
                    "value object constructors require exactly one input parameter",
                ));
            }
            input
        }
    };
    Ok(parsed(None, input, signature))
}

pub fn parse_aggregate(signature: &Signature) -> syn::Result<ParsedSignature> {
    parse_aggregate_root(signature, true)
}

pub fn parse_aggregate_instance(signature: &Signature) -> syn::Result<ParsedSignature> {
    let Some(FnArg::Receiver(receiver)) = signature.inputs.first() else {
        return Err(syn::Error::new_spanned(
            &signature.ident,
            "executable aggregate actions require an &mut self receiver",
        ));
    };
    if receiver.colon_token.is_some() {
        return Err(syn::Error::new_spanned(
            receiver,
            "executable aggregate actions do not support typed receivers",
        ));
    }
    let Some((_, lifetime)) = &receiver.reference else {
        return Err(syn::Error::new_spanned(
            receiver,
            "executable aggregate actions require an &mut self receiver",
        ));
    };
    if receiver.mutability.is_none() {
        return Err(syn::Error::new_spanned(
            receiver,
            "executable aggregate actions require a mutable &mut self receiver",
        ));
    }
    if let Some(lifetime) = lifetime {
        return Err(syn::Error::new_spanned(
            lifetime,
            "executable aggregate action receivers cannot have an explicit lifetime",
        ));
    }
    let input = parse_business_inputs(signature.inputs.iter().skip(1), 1, "aggregate")?;
    let parsed = parsed(None, input, signature);
    if !is_unit(&parsed.output) {
        return Err(syn::Error::new_spanned(
            &signature.output,
            "executable aggregate actions must return () or Result<(), DomainError>",
        ));
    }
    Ok(parsed)
}

fn parse_aggregate_root(signature: &Signature, mutable: bool) -> syn::Result<ParsedSignature> {
    let Some(FnArg::Typed(root)) = signature.inputs.first() else {
        return Err(syn::Error::new_spanned(
            &signature.ident,
            if mutable {
                "aggregate actions require a first `root: &mut RootType` parameter"
            } else {
                "executable aggregate actions require a first `root: &RootType` parameter"
            },
        ));
    };
    validate_pattern(&root.pat, "root")?;
    let Type::Reference(reference) = root.ty.as_ref() else {
        return Err(syn::Error::new_spanned(
            &root.ty,
            "aggregate root must have type &mut RootType",
        ));
    };
    if mutable && reference.mutability.is_none() {
        return Err(syn::Error::new_spanned(
            &root.ty,
            "aggregate root must be mutable with type &mut RootType",
        ));
    }
    if !mutable && reference.mutability.is_some() {
        return Err(syn::Error::new_spanned(
            &root.ty,
            "executable aggregate action roots must be immutable with type &RootType",
        ));
    }
    if reference.lifetime.is_some() {
        return Err(syn::Error::new_spanned(
            &root.ty,
            "aggregate root cannot have an explicit lifetime",
        ));
    }
    let input = parse_business_inputs(signature.inputs.iter().skip(1), 1, "aggregate")?;
    Ok(parsed(Some((*reference.elem).clone()), input, signature))
}

fn parse_business_inputs<'a>(
    inputs: impl Iterator<Item = &'a FnArg>,
    maximum: usize,
    owner: &str,
) -> syn::Result<Option<Type>> {
    let inputs: Vec<_> = inputs.collect();
    if inputs.len() > maximum {
        return Err(syn::Error::new_spanned(
            inputs[maximum],
            format!(
                "{owner} actions accept at most one business input; group multiple values into one type"
            ),
        ));
    }
    inputs.first().map(|input| parse_input(input)).transpose()
}

fn parse_input(input: &FnArg) -> syn::Result<Type> {
    let FnArg::Typed(input) = input else {
        return Err(syn::Error::new_spanned(input, "unexpected receiver"));
    };
    validate_pattern(&input.pat, "input")?;
    Ok((*input.ty).clone())
}

fn validate_pattern(pattern: &Pat, expected: &str) -> syn::Result<()> {
    let Pat::Ident(pattern) = pattern else {
        return Err(syn::Error::new_spanned(
            pattern,
            format!("action parameter must be a simple identifier named `{expected}`"),
        ));
    };
    if pattern.by_ref.is_some() || pattern.subpat.is_some() || pattern.ident != expected {
        return Err(syn::Error::new_spanned(
            pattern,
            format!("action parameter must be a simple identifier named `{expected}`"),
        ));
    }
    Ok(())
}

fn parsed(root: Option<Type>, input: Option<Type>, signature: &Signature) -> ParsedSignature {
    let output = match &signature.output {
        ReturnType::Default => syn::parse_quote_spanned!(signature.span()=> ()),
        ReturnType::Type(_, output) => (**output).clone(),
    };
    let (output, error) = split_result(output);
    ParsedSignature {
        root,
        input,
        output,
        error,
    }
}

fn split_result(output: Type) -> (Type, Option<Type>) {
    let Type::Path(TypePath { qself: None, path }) = &output else {
        return (output, None);
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
        return (output, None);
    }
    if path
        .segments
        .iter()
        .take(path.segments.len() - 1)
        .any(|segment| !matches!(segment.arguments, PathArguments::None))
    {
        return (output, None);
    }
    let PathArguments::AngleBracketed(arguments) = &path.segments.last().unwrap().arguments else {
        return (output, None);
    };
    let types: Vec<_> = arguments
        .args
        .iter()
        .filter_map(|argument| match argument {
            GenericArgument::Type(ty) => Some(ty.clone()),
            _ => None,
        })
        .collect();
    if arguments.args.len() == 2 && types.len() == 2 {
        (types[0].clone(), Some(types[1].clone()))
    } else {
        (output, None)
    }
}

fn is_unit(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}
