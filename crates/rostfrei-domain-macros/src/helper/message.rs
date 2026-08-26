use syn::{LitStr, Result};

pub fn validate(message: &LitStr) -> Result<()> {
    if message.value().trim().is_empty() {
        Err(syn::Error::new(message.span(), "message must not be empty"))
    } else {
        Ok(())
    }
}
