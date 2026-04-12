use syn::{LitStr, Result};

pub fn validate(code: &LitStr) -> Result<()> {
    let value = code.value();
    let mut bytes = value.bytes();
    let valid = bytes.next().is_some_and(|byte| byte.is_ascii_uppercase())
        && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');

    if valid {
        Ok(())
    } else {
        Err(syn::Error::new(
            code.span(),
            "code must be SCREAMING_SNAKE_CASE and begin with an ASCII uppercase letter",
        ))
    }
}
