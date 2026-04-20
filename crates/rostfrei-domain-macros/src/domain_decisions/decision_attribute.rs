use syn::{Attribute, ImplItemFn, LitStr};

pub struct DecisionAttribute {
    pub id: LitStr,
    pub label: LitStr,
}

pub fn extract(method: &mut ImplItemFn) -> syn::Result<Option<DecisionAttribute>> {
    let position = {
        let mut positions = method
            .attrs
            .iter()
            .enumerate()
            .filter(|(_, attribute)| attribute.path().is_ident("decision"));
        let Some((position, _)) = positions.next() else {
            return Ok(None);
        };
        if let Some((_, duplicate)) = positions.next() {
            return Err(syn::Error::new_spanned(
                duplicate,
                "duplicate decision attribute",
            ));
        }
        position
    };
    parse(&method.attrs.remove(position)).map(Some)
}

fn parse(attribute: &Attribute) -> syn::Result<DecisionAttribute> {
    let mut id = None;
    let mut label = None;
    attribute.parse_nested_meta(|meta| {
        let target = if meta.path.is_ident("id") {
            &mut id
        } else if meta.path.is_ident("label") {
            &mut label
        } else {
            return Err(meta.error("unsupported decision attribute metadata"));
        };
        if target.is_some() {
            return Err(meta.error(if meta.path.is_ident("id") {
                "duplicate id"
            } else {
                "duplicate label"
            }));
        }
        *target = Some(meta.value()?.parse::<LitStr>()?);
        Ok(())
    })?;
    Ok(DecisionAttribute {
        id: id.ok_or_else(|| syn::Error::new_spanned(attribute, "missing id"))?,
        label: label.ok_or_else(|| syn::Error::new_spanned(attribute, "missing label"))?,
    })
}
