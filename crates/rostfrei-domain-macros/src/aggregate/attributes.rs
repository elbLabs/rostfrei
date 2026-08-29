use syn::{Attribute, LitStr, Path, Result, TypePath};

pub struct Attributes {
    pub id: LitStr,
    pub label: LitStr,
    pub context: TypePath,
    pub root: TypePath,
    pub actions: Vec<Path>,
    pub decisions: Vec<Path>,
    pub invariants: Vec<Path>,
    pub events: Option<Vec<Path>>,
}

impl Attributes {
    pub fn parse(attributes: &[Attribute]) -> Result<Self> {
        let domain = crate::helper::domain_attribute::locate(attributes)?;
        let mut id = None;
        let mut label = None;
        let mut context = None;
        let mut root = None;
        let mut actions = None;
        let mut decisions = None;
        let mut invariants = None;
        let mut events = None;

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
            if meta.path.is_ident("context") {
                if context.is_some() {
                    return Err(meta.error("duplicate context"));
                }
                context = Some(meta.value()?.parse::<TypePath>()?);
                return Ok(());
            }
            if meta.path.is_ident("root") {
                if root.is_some() {
                    return Err(meta.error("duplicate root"));
                }
                root = Some(meta.value()?.parse::<TypePath>()?);
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
            if meta.path.is_ident("events") {
                if events.is_some() {
                    return Err(meta.error("duplicate events"));
                }
                events = Some(crate::helper::event_paths::parse(meta.value()?)?);
                return Ok(());
            }
            Err(meta.error("unsupported domain attribute"))
        })?;

        let id = id.ok_or_else(|| syn::Error::new_spanned(domain, "missing id"))?;
        let label = label.ok_or_else(|| syn::Error::new_spanned(domain, "missing label"))?;
        let context = context.ok_or_else(|| syn::Error::new_spanned(domain, "missing context"))?;
        let root = root.ok_or_else(|| syn::Error::new_spanned(domain, "missing root"))?;
        Ok(Self {
            id,
            label,
            context,
            root,
            actions: actions.unwrap_or_default(),
            decisions: decisions.unwrap_or_default(),
            invariants: invariants.unwrap_or_default(),
            events,
        })
    }
}
