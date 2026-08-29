use syn::{Attribute, LitStr, Path, PathArguments, Result, TypePath};

pub struct Attributes {
    pub id: LitStr,
    pub label: LitStr,
    pub owner: TypePath,
    pub actions: Vec<Path>,
    pub decisions: Vec<Path>,
    pub invariants: Vec<Path>,
    pub lifecycle: Option<TypePath>,
}

impl Attributes {
    pub fn parse(attributes: &[Attribute]) -> Result<Self> {
        let domain = crate::helper::domain_attribute::locate(attributes)?;
        let mut id = None;
        let mut label = None;
        let mut owner = None;
        let mut actions = None;
        let mut decisions = None;
        let mut invariants = None;
        let mut lifecycle = None;
        domain.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                if id.is_some() {
                    return Err(meta.error("duplicate id"));
                }
                id = Some(meta.value()?.parse::<LitStr>()?);
                return Ok(());
            }
            if meta.path.is_ident("label") {
                if label.is_some() {
                    return Err(meta.error("duplicate label"));
                }
                label = Some(meta.value()?.parse::<LitStr>()?);
                return Ok(());
            }
            if meta.path.is_ident("owner") {
                if owner.is_some() {
                    return Err(meta.error("duplicate owner"));
                }
                owner = Some(meta.value()?.parse::<TypePath>()?);
                return Ok(());
            }
            if meta.path.is_ident("actions") {
                if actions.is_some() {
                    return Err(meta.error("duplicate actions"));
                }
                actions = Some(crate::helper::action_paths::parse(meta.value()?)?);
                return Ok(());
            }
            if meta.path.is_ident("decisions") {
                if decisions.is_some() {
                    return Err(meta.error("duplicate decisions"));
                }
                if !meta.input.peek(syn::Token![=]) {
                    return Err(meta.error(
                        "bare `decisions` is no longer supported; use `decisions = [GroupA, module::GroupB]`",
                    ));
                }
                decisions = Some(crate::helper::decision_group_paths::parse(meta.value()?)?);
                return Ok(());
            }
            if meta.path.is_ident("invariants") {
                if invariants.is_some() {
                    return Err(meta.error("duplicate invariants"));
                }
                invariants = Some(crate::helper::invariant_paths::parse(meta.value()?)?);
                return Ok(());
            }
            if meta.path.is_ident("lifecycle") {
                if lifecycle.is_some() {
                    return Err(meta.error("duplicate lifecycle"));
                }
                let path = meta.value()?.parse::<TypePath>()?;
                validate_lifecycle_path(&path)?;
                lifecycle = Some(path);
                return Ok(());
            }
            Err(meta.error("unsupported domain attribute"))
        })?;
        let id = id.ok_or_else(|| syn::Error::new_spanned(domain, "missing id"))?;
        let label = label.ok_or_else(|| syn::Error::new_spanned(domain, "missing label"))?;
        let owner = owner.ok_or_else(|| syn::Error::new_spanned(domain, "missing owner"))?;
        Ok(Self {
            id,
            label,
            owner,
            actions: actions.unwrap_or_default(),
            decisions: decisions.unwrap_or_default(),
            invariants: invariants.unwrap_or_default(),
            lifecycle,
        })
    }
}

fn validate_lifecycle_path(path: &TypePath) -> Result<()> {
    if path.qself.is_some()
        || path
            .path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, PathArguments::None))
    {
        return Err(syn::Error::new_spanned(
            path,
            "lifecycle must be a direct, non-generic type path",
        ));
    }
    Ok(())
}
