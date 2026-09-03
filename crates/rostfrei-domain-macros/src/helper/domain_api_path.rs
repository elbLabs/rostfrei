use syn::Path;

pub fn resolve() -> Path {
    syn::parse_quote!(crate::__rostfrei_macro_support)
}
