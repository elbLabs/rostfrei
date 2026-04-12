use syn::{LitStr, Result};

pub fn validate(id: &LitStr) -> Result<()> {
    let value = id.value();
    let valid = !value.is_empty()
        && value.split('-').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        });

    if valid {
        Ok(())
    } else {
        Err(syn::Error::new(
            id.span(),
            "id must be nonempty lowercase kebab-case using ASCII letters and digits",
        ))
    }
}
