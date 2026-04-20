use syn::visit_mut::VisitMut;
use syn::{FnArg, GenericParam, Pat, ReturnType, Signature, Type, TypeReference};

use super::decision::Parameter;

pub struct DecisionTypes {
    pub parameters: Vec<Parameter>,
    pub return_type: Type,
}

pub fn parse(signature: &Signature) -> syn::Result<DecisionTypes> {
    validate_qualifiers(signature)?;
    let parameters = parse_parameters(signature)?;
    let return_type = parse_return_type(signature)?;
    Ok(DecisionTypes {
        parameters,
        return_type,
    })
}

fn validate_qualifiers(signature: &Signature) -> syn::Result<()> {
    if signature.variadic.is_some()
        || signature.asyncness.is_some()
        || signature.unsafety.is_some()
        || signature.abi.is_some()
        || signature.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(
            signature,
            "decisions cannot be async, unsafe, extern, variadic, or have where clauses",
        ));
    }
    if let Some(parameter) = signature.generics.params.first() {
        let message = match parameter {
            GenericParam::Lifetime(_) => {
                "decisions cannot declare named lifetime generics; use an elided reference or an explicit `'static` reference"
            }
            GenericParam::Type(_) | GenericParam::Const(_) => {
                "decisions cannot declare type or const generics"
            }
        };
        return Err(syn::Error::new_spanned(parameter, message));
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
    let descriptor_type = parameter_descriptor_type(&input.ty)?;
    Ok(Parameter {
        name,
        signature_type: (*input.ty).clone(),
        descriptor_type,
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

fn parameter_descriptor_type(authored: &Type) -> syn::Result<Type> {
    if let Type::Reference(reference) = transparent_type(authored) {
        referenced_parameter_type(reference)
    } else {
        reject_nested_reference(authored)?;
        Ok(authored.clone())
    }
}

fn referenced_parameter_type(reference: &TypeReference) -> syn::Result<Type> {
    if reference.mutability.is_some() {
        return Err(syn::Error::new_spanned(
            reference,
            "decision parameters cannot use mutable references; use `&T` or owned `T`",
        ));
    }
    if let Some(lifetime) = &reference.lifetime
        && lifetime.ident != "static"
    {
        return Err(syn::Error::new_spanned(
            lifetime,
            "decision parameter references must use an elided lifetime or explicit `'static`",
        ));
    }
    reject_nested_reference(&reference.elem)?;
    Ok((*reference.elem).clone())
}

fn transparent_type(mut ty: &Type) -> &Type {
    loop {
        ty = match ty {
            Type::Group(group) => &group.elem,
            Type::Paren(paren) => &paren.elem,
            _ => return ty,
        };
    }
}

fn reject_nested_reference(ty: &Type) -> syn::Result<()> {
    let mut finder = ReferenceFinder::default();
    let mut ty = ty.clone();
    finder.visit_type_mut(&mut ty);
    finder.reference.map_or(Ok(()), |reference| {
        Err(syn::Error::new_spanned(
            reference,
            "decision parameter types cannot contain nested references",
        ))
    })
}

#[derive(Default)]
struct ReferenceFinder {
    reference: Option<TypeReference>,
}

impl VisitMut for ReferenceFinder {
    fn visit_type_reference_mut(&mut self, reference: &mut TypeReference) {
        if self.reference.is_none() {
            self.reference = Some(reference.clone());
        }
    }
}

fn parse_return_type(signature: &Signature) -> syn::Result<Type> {
    let return_type = match &signature.output {
        ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &signature.ident,
                "decisions must declare an explicit owned return type implementing DecisionOutcomeType",
            ));
        }
        ReturnType::Type(_, return_type) => return_type.as_ref(),
    };
    reject_return_references(return_type)?;
    Ok(return_type.clone())
}

fn reject_return_references(return_type: &Type) -> syn::Result<()> {
    let mut finder = ReferenceFinder::default();
    let mut return_type = return_type.clone();
    finder.visit_type_mut(&mut return_type);
    finder.reference.map_or(Ok(()), |reference| {
        Err(syn::Error::new_spanned(
            reference,
            "decision return types must be owned and cannot contain references",
        ))
    })
}
