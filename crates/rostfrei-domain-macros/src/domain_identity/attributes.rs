use syn::{Attribute, PathArguments, TypePath};

pub struct Attributes {
    pub owner: TypePath,
    pub scalar: Option<TypePath>,
}

pub fn parse(attributes: &[Attribute]) -> syn::Result<Attributes> {
    let mut domain = attributes
        .iter()
        .filter(|attribute| crate::helper::domain_attribute::is_helper(attribute));
    let Some(attribute) = domain.next() else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "DomainIdentity requires #[domain(owner = EntityType)]",
        ));
    };
    if let Some(duplicate) = domain.next() {
        return Err(syn::Error::new_spanned(
            duplicate,
            "DomainIdentity supports exactly one domain attribute",
        ));
    }
    let mut owner = None;
    let mut scalar = None;
    attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("owner") {
            if owner.is_some() {
                return Err(meta.error("duplicate owner"));
            }
            let parsed = meta.value()?.parse::<TypePath>()?;
            if !is_direct_non_generic(&parsed) {
                return Err(meta
                    .error("DomainIdentity owner must be a direct, non-generic entity type path"));
            }
            owner = Some(parsed);
            return Ok(());
        }
        if meta.path.is_ident("scalar") {
            if scalar.is_some() {
                return Err(meta.error("duplicate scalar"));
            }
            let parsed = meta.value()?.parse::<TypePath>()?;
            if !is_direct_non_generic(&parsed) {
                return Err(meta.error(
                    "DomainIdentity scalar provider must be a direct, non-generic type path",
                ));
            }
            scalar = Some(parsed);
            return Ok(());
        }
        Err(meta.error("unsupported DomainIdentity attribute; expected owner or scalar"))
    })?;
    let owner = owner.ok_or_else(|| syn::Error::new_spanned(attribute, "missing owner"))?;
    Ok(Attributes { owner, scalar })
}

fn is_direct_non_generic(path: &TypePath) -> bool {
    path.qself.is_none()
        && path
            .path
            .segments
            .iter()
            .all(|segment| matches!(segment.arguments, PathArguments::None))
}
