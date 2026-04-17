use syn::{GenericArgument, PathArguments, Result, Type, TypePath};

use super::ir::Wrapper;

pub fn parse(ty: &Type) -> Result<(Vec<Wrapper>, TypePath)> {
    let mut wrappers = Vec::new();
    let mut current = ty;
    loop {
        let Type::Path(path) = current else {
            return Err(syn::Error::new_spanned(
                current,
                "field must use a supported scalar or direct domain type",
            ));
        };
        if path.qself.is_some() {
            return Err(syn::Error::new_spanned(
                path,
                "field must use a direct type path",
            ));
        }
        if let Some(wrapper) = wrapper(path) {
            let segment = path.path.segments.last().unwrap();
            let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                return Err(syn::Error::new_spanned(
                    segment,
                    "Vec and Option require exactly one plain type argument",
                ));
            };
            if arguments.args.len() != 1 {
                return Err(syn::Error::new_spanned(
                    arguments,
                    "Vec and Option require exactly one plain type argument",
                ));
            }
            let Some(GenericArgument::Type(inner)) = arguments.args.first() else {
                return Err(syn::Error::new_spanned(
                    arguments,
                    "Vec and Option require exactly one plain type argument",
                ));
            };
            wrappers.push(wrapper);
            current = inner;
            continue;
        }
        if path
            .path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, PathArguments::None))
        {
            return Err(syn::Error::new_spanned(
                path,
                "field base type must be direct and non-generic",
            ));
        }
        return Ok((wrappers, path.clone()));
    }
}

fn wrapper(path: &TypePath) -> Option<Wrapper> {
    let segments: Vec<_> = path.path.segments.iter().collect();
    let (name, modules) = segments.split_last()?;
    let name = name.ident.to_string();
    let modules: Vec<_> = modules
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    match (modules.as_slice(), name.as_str()) {
        ([], "Vec") => Some(Wrapper::List),
        ([root, module], "Vec") if root == "std" && module == "vec" => Some(Wrapper::List),
        ([root, module], "Vec") if root == "alloc" && module == "vec" => Some(Wrapper::List),
        ([], "Option") => Some(Wrapper::Optional),
        ([root, module], "Option") if (root == "std" || root == "core") && module == "option" => {
            Some(Wrapper::Optional)
        }
        _ => None,
    }
}
