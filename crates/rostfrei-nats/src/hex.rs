pub(crate) fn encode_lower_hex(bytes: impl IntoIterator<Item = u8>) -> String {
    let bytes = bytes.into_iter();
    let capacity = bytes.size_hint().0.saturating_mul(2);
    let mut encoded = String::with_capacity(capacity);
    for byte in bytes {
        encoded.push(lower_hex_digit(byte >> 4));
        encoded.push(lower_hex_digit(byte & 0x0f));
    }
    encoded
}

fn lower_hex_digit(nibble: u8) -> char {
    let byte = if nibble < 10 {
        b'0'.saturating_add(nibble)
    } else {
        b'a'.saturating_add(nibble.saturating_sub(10))
    };
    char::from(byte)
}
