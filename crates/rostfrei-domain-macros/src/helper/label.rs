use syn::{LitStr, Result};

pub fn validate(label: &LitStr) -> Result<()> {
    if label.value().trim().is_empty() {
        Err(syn::Error::new(label.span(), "label must not be empty"))
    } else {
        Ok(())
    }
}
