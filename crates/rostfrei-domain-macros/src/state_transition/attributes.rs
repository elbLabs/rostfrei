use syn::{Attribute, TypePath};

pub struct Attributes {
    pub state: TypePath,
}

pub fn parse(attributes: &[Attribute]) -> syn::Result<Attributes> {
    let transitions: Vec<_> = attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("transition"))
        .collect();
    let Some(transition) = transitions.first() else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "missing transition attribute",
        ));
    };
    if let Some(duplicate) = transitions.get(1) {
        return Err(syn::Error::new_spanned(
            duplicate,
            "duplicate transition attribute",
        ));
    }

    let mut state = None;
    transition.parse_nested_meta(|meta| {
        if meta.path.is_ident("state") {
            if state.is_some() {
                return Err(meta.error("duplicate state type"));
            }
            state = Some(meta.value()?.parse::<TypePath>()?);
            return Ok(());
        }
        Err(meta.error("unsupported state transition attribute"))
    })?;

    Ok(Attributes {
        state: state.ok_or_else(|| syn::Error::new_spanned(transition, "missing state type"))?,
    })
}
