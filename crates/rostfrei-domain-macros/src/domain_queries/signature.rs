use syn::{FnArg, Pat, ReturnType, Signature, Type, Visibility};

pub struct ParsedSignature {
    pub root: Type,
    pub input: Option<Type>,
    pub output: Type,
}

pub fn parse(signature: &Signature, visibility: &Visibility) -> syn::Result<ParsedSignature> {
    if !matches!(visibility, Visibility::Public(_)) {
        return Err(syn::Error::new_spanned(
            visibility,
            "queries must be public",
        ));
    }
    if matches!(signature.inputs.first(), Some(FnArg::Receiver(_))) {
        return Err(syn::Error::new_spanned(
            &signature.inputs,
            "queries must be associated functions without a receiver",
        ));
    }
    if signature.inputs.is_empty() {
        return Err(syn::Error::new_spanned(
            &signature.inputs,
            "queries require a root parameter",
        ));
    }
    if signature.inputs.len() > 2 {
        return Err(syn::Error::new_spanned(
            signature.inputs.iter().nth(2).unwrap(),
            "queries accept at most one input parameter",
        ));
    }
    let root = borrowed(signature.inputs.first().unwrap(), "root", "query root")?;
    let input = signature
        .inputs
        .iter()
        .nth(1)
        .map(|input| borrowed(input, "input", "query input"))
        .transpose()?;
    let ReturnType::Type(_, output) = &signature.output else {
        return Err(syn::Error::new_spanned(
            &signature.output,
            "queries require an owned output",
        ));
    };
    if matches!(output.as_ref(), Type::Reference(_))
        || matches!(output.as_ref(), Type::Tuple(tuple) if tuple.elems.is_empty())
    {
        return Err(syn::Error::new_spanned(
            output,
            "queries require a non-unit owned output",
        ));
    }
    Ok(ParsedSignature {
        root,
        input,
        output: (**output).clone(),
    })
}

fn borrowed(input: &FnArg, name: &str, subject: &str) -> syn::Result<Type> {
    let FnArg::Typed(input) = input else {
        return Err(syn::Error::new_spanned(input, "unexpected receiver"));
    };
    let Pat::Ident(pattern) = input.pat.as_ref() else {
        return Err(syn::Error::new_spanned(
            &input.pat,
            format!("query parameter must be a simple identifier named `{name}`"),
        ));
    };
    if pattern.by_ref.is_some() || pattern.subpat.is_some() || pattern.ident != name {
        return Err(syn::Error::new_spanned(
            &input.pat,
            format!("query parameter must be a simple identifier named `{name}`"),
        ));
    }
    let Type::Reference(reference) = input.ty.as_ref() else {
        return Err(syn::Error::new_spanned(
            &input.ty,
            format!("{subject} must be an immutable reference without an explicit lifetime"),
        ));
    };
    if reference.mutability.is_some() || reference.lifetime.is_some() {
        return Err(syn::Error::new_spanned(
            &input.ty,
            format!("{subject} must be an immutable reference without an explicit lifetime"),
        ));
    }
    Ok((*reference.elem).clone())
}
