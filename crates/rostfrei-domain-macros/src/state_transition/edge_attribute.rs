use syn::{Ident, Variant};

pub struct EdgeAttribute {
    pub from: Ident,
    pub to: Ident,
}

pub fn parse(variant: &Variant) -> syn::Result<Vec<EdgeAttribute>> {
    let edges: Vec<_> = variant
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("edge"))
        .collect();
    if edges.is_empty() {
        return Err(syn::Error::new_spanned(
            &variant.ident,
            "StateTransition variants require at least one edge attribute",
        ));
    }

    edges.into_iter().map(parse_edge).collect()
}

fn parse_edge(edge: &syn::Attribute) -> syn::Result<EdgeAttribute> {
    let mut from = None;
    let mut to = None;
    edge.parse_nested_meta(|meta| {
        if meta.path.is_ident("from") {
            if from.is_some() {
                return Err(meta.error("duplicate from state"));
            }
            from = Some(meta.value()?.parse::<Ident>()?);
            return Ok(());
        }
        if meta.path.is_ident("to") {
            if to.is_some() {
                return Err(meta.error("duplicate to state"));
            }
            to = Some(meta.value()?.parse::<Ident>()?);
            return Ok(());
        }
        Err(meta.error("unsupported state transition edge metadata"))
    })?;

    Ok(EdgeAttribute {
        from: from.ok_or_else(|| syn::Error::new_spanned(edge, "missing from state"))?,
        to: to.ok_or_else(|| syn::Error::new_spanned(edge, "missing to state"))?,
    })
}
