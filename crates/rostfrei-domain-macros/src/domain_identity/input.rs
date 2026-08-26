use syn::{Data, DeriveInput, Fields, PathArguments, Result, TypePath};

pub fn extract(input: &DeriveInput) -> Result<TypePath> {
    validate_generics(input)?;
    extract_newtype(input)
}

fn validate_generics(input: &DeriveInput) -> Result<()> {
    if input.generics.params.is_empty() {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            &input.generics,
            "DomainIdentity only supports non-generic tuple structs with exactly one field",
        ))
    }
}

fn extract_newtype(input: &DeriveInput) -> Result<TypePath> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "DomainIdentity only supports non-generic tuple structs with exactly one field",
        ));
    };
    let Fields::Unnamed(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "DomainIdentity only supports non-generic tuple structs with exactly one field",
        ));
    };
    if fields.unnamed.len() != 1 {
        return Err(syn::Error::new_spanned(
            fields,
            "DomainIdentity only supports non-generic tuple structs with exactly one field",
        ));
    }
    let field = fields.unnamed.first().unwrap();
    if let Some(attribute) = field
        .attrs
        .iter()
        .find(|attribute| crate::helper::domain_attribute::is_helper(attribute))
    {
        return Err(syn::Error::new_spanned(
            attribute,
            "DomainIdentity fields do not support domain attributes",
        ));
    }
    let syn::Type::Path(path) = &field.ty else {
        return Err(syn::Error::new_spanned(
            &field.ty,
            "DomainIdentity field must be a supported canonical scalar",
        ));
    };
    Ok(path.clone())
}

pub fn validate_value(path: &TypePath, has_semantic_scalar: bool) -> Result<()> {
    if has_semantic_scalar {
        if path.qself.is_some()
            || path
                .path
                .segments
                .iter()
                .any(|segment| !matches!(segment.arguments, PathArguments::None))
        {
            return Err(syn::Error::new_spanned(
                path,
                "DomainIdentity semantic scalar value must be a direct, non-generic type path",
            ));
        }
    } else if crate::field::classify_scalar(path).is_none() {
        return Err(syn::Error::new_spanned(
            path,
            "DomainIdentity field must be a supported canonical scalar",
        ));
    }
    Ok(())
}
