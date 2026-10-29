//! Canonical, readable identifier tokens for NATS event subjects.
//!
//! Business identifiers remain in their envelopes. Escaping every byte except
//! ASCII letters, digits, hyphens and underscores prevents a literal identifier
//! from changing subject boundaries or becoming a wildcard. `~` denotes absence.

pub fn encode(value: &str) -> String {
    let mut token = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') {
            token.push(char::from(byte));
        } else {
            token.push('%');
            token.push(hex_digit(byte >> 4));
            token.push(hex_digit(byte & 15));
        }
    }
    token
}

fn hex_digit(nibble: u8) -> char {
    char::from(if nibble < 10 {
        b'0'.saturating_add(nibble)
    } else {
        b'A'.saturating_add(nibble.saturating_sub(10))
    })
}

pub fn optional(value: Option<&str>) -> String {
    value.map_or_else(|| "~".to_owned(), encode)
}

/// Rejects noncanonical encodings as well as wildcard and missing-value tokens.
pub fn decode(token: &str) -> Option<String> {
    if token.is_empty() {
        return None;
    }
    let decoded = percent_encoding::percent_decode_str(token)
        .decode_utf8()
        .ok()?;
    (encode(&decoded) == token).then(|| decoded.into_owned())
}

#[cfg(test)]
mod tests {
    use super::{decode, encode, optional};

    #[test]
    fn identifiers_are_readable_without_changing_subject_boundaries() {
        for (plain, token) in [
            ("R1", "R1"),
            ("order-O1_2", "order-O1_2"),
            ("ordering/order", "ordering%2Forder"),
            ("a.b.*.>", "a%2Eb%2E%2A%2E%3E"),
            ("a%2Eb", "a%252Eb"),
            ("~", "%7E"),
            ("customer 7", "customer%207"),
            ("Fahrrad-ä", "Fahrrad-%C3%A4"),
        ] {
            assert_eq!(encode(plain), token);
            assert_eq!(decode(token).as_deref(), Some(plain));
            assert!(!token.contains(['.', '*', '>', ' ']));
        }
    }

    #[test]
    fn missing_identity_cannot_alias_a_real_identifier() {
        assert_eq!(optional(None), "~");
        assert_eq!(optional(Some("~")), "%7E");
        assert_eq!(optional(Some("R1")), "R1");
    }

    #[test]
    fn only_canonical_literal_tokens_are_accepted() {
        for token in ["", "~", "a.b", "*", ">", "%", "%GG", "%FF", "%2f", "%41"] {
            assert!(decode(token).is_none(), "accepted {token:?}");
        }
    }
}
