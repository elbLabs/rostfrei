use syn::{Attribute, PathArguments, TypePath};

pub struct Attributes {
    pub owner: TypePath,
    pub scalar: Option<TypePath>,
}

pub fn parse(attributes: &[Attribute]) -> syn::Result<Attributes> {
    let domain: Vec<_> = attributes
        .iter()
        .filter(|attribute| crate::helper::domain_attribute::is_helper(attribute))
        .collect();
    if domain.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "DomainIdentity requires #[domain(owner = EntityType)]",
        ));
    }
    if domain.len() > 1 {
        return Err(syn::Error::new_spanned(
            domain[1],
            "DomainIdentity supports exactly one domain attribute",
        ));
    }
    let attribute = domain[0];
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
