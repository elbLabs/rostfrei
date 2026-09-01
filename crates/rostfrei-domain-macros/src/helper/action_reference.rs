use proc_macro2::Ident;
use syn::LitStr;

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
