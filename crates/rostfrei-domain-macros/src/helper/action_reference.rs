use proc_macro2::Ident;
use syn::LitStr;
use syn::ext::IdentExt;

const HIDDEN_PREFIX: &str = "__DOMAIN_ACTION_REFERENCE_";

pub fn public_name(action_id: &LitStr) -> String {
    let mut name = action_id.value().to_ascii_uppercase().replace('-', "_");
    if name.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        name.insert(0, '_');
    }
    name
}

pub fn hidden_from_action_id(action_id: &LitStr) -> Ident {
    Ident::new(
        &format!("{HIDDEN_PREFIX}{}", public_name(action_id)),
        action_id.span(),
    )
}

pub fn hidden_from_public(reference: &Ident) -> Ident {
    Ident::new(
        &format!("{HIDDEN_PREFIX}{}", reference.unraw()),
        reference.span(),
    )
}

pub fn is_hidden(reference: &Ident) -> bool {
    reference.unraw().to_string().starts_with(HIDDEN_PREFIX)
}

pub fn is_canonical_public(reference: &Ident) -> bool {
    let name = reference.unraw().to_string();
    let normalized = if let Some(numeric) = name.strip_prefix('_') {
        if !numeric.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            return false;
        }
        numeric
    } else {
        if !name.as_bytes().first().is_some_and(u8::is_ascii_uppercase) {
            return false;
        }
        name.as_str()
    };
    normalized.split('_').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    })
}
