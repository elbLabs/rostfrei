use proc_macro2::Ident;
use syn::LitStr;

const HIDDEN_PREFIX: &str = "__DOMAIN_INVARIANT_REFERENCE_";

pub fn hidden_from_invariant_id(invariant_id: &LitStr) -> Ident {
    let mut name = invariant_id.value().to_ascii_uppercase().replace('-', "_");
    if name.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        name.insert(0, '_');
    }
    Ident::new(&format!("{HIDDEN_PREFIX}{name}"), invariant_id.span())
}
