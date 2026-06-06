//! Configuration shared by real-service integration tests.

use std::{env, io};

/// Require an explicitly configured broker instead of silently skipping tests.
pub fn nats_url() -> io::Result<String> {
    configured_nats_url(env::var("ROSTFREI_NATS_URL").ok().as_deref())
}

fn configured_nats_url(value: Option<&str>) -> io::Result<String> {
    value
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "ROSTFREI_NATS_URL must be set and nonempty for real-NATS tests; \
                 run `python3 scripts/test_nats.py` to provision a disposable broker",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::configured_nats_url;

    #[test]
    fn missing_or_empty_configuration_is_an_error() {
        for value in [None, Some(""), Some(" \t\n")] {
            assert!(configured_nats_url(value).is_err());
        }
    }

    #[test]
    fn explicit_configuration_is_trimmed() {
        assert!(matches!(
            configured_nats_url(Some(" nats://127.0.0.1:4222 ")),
            Ok(url) if url == "nats://127.0.0.1:4222"
        ));
    }
}
