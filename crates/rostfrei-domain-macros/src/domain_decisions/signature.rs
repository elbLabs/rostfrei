use syn::{FnArg, GenericParam, ReturnType, Signature, Type};

pub struct DecisionTypes {
    pub parameters: Vec<Type>,
    pub return_type: Type,
}

pub fn parse(signature: &Signature) -> syn::Result<DecisionTypes> {
    validate_qualifiers(signature)?;
    let parameters = signature
        .inputs
        .iter()
        .map(parameter_type)
        .collect::<syn::Result<_>>()?;
    let return_type = match &signature.output {
        ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &signature.ident,
                "decisions must declare an explicit return type implementing DecisionOutcomeType",
            ));
        }
        ReturnType::Type(_, return_type) => (**return_type).clone(),
    };
    Ok(DecisionTypes {
        parameters,
        return_type,
    })
}

fn parameter_type(input: &FnArg) -> syn::Result<Type> {
    match input {
        FnArg::Typed(input) => Ok((*input.ty).clone()),
        FnArg::Receiver(receiver) => Err(syn::Error::new_spanned(
            receiver,
            "decisions must be associated functions without a receiver",
        )),
    }
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
