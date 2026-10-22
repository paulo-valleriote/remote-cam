//! Lowercase hexadecimal helpers shared by the contract consumer and its vectors.

/// Decodes exactly `byte_len` bytes from a lowercase hexadecimal string.
///
/// Returns `None` when the input has the wrong length, contains an uppercase
/// digit, or contains a character that is not a hexadecimal digit.
pub fn decode_hex_exact(input: &str, byte_len: usize) -> Option<Vec<u8>> {
    if input.len() != byte_len * 2 {
        return None;
    }
    let bytes = input.as_bytes();
    let (pairs, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    let mut output = Vec::with_capacity(byte_len);
    for pair in pairs {
        let high = decode_nibble(pair[0])?;
        let low = decode_nibble(pair[1])?;
        output.push((high << 4) | low);
    }
    Some(output)
}

/// Decodes a lowercase hexadecimal string of any even length.
pub fn decode_hex(input: &str) -> Option<Vec<u8>> {
    if !input.len().is_multiple_of(2) {
        return None;
    }
    decode_hex_exact(input, input.len() / 2)
}

/// Encodes bytes as lowercase hexadecimal.
pub fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[usize::from(byte >> 4)] as char);
        output.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    output
}

fn decode_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_lowercase_hex() {
        let bytes = [0x00, 0x0f, 0xa5, 0xff];
        let encoded = encode_hex(&bytes);
        assert_eq!(encoded, "000fa5ff");
        assert_eq!(decode_hex_exact(&encoded, 4), Some(bytes.to_vec()));
    }

    #[test]
    fn rejects_uppercase_odd_and_non_hex() {
        assert_eq!(decode_hex_exact("AA", 1), None);
        assert_eq!(decode_hex("abc"), None);
        assert_eq!(decode_hex("zz"), None);
    }

    #[test]
    fn enforces_the_exact_byte_length() {
        assert_eq!(decode_hex_exact("0000", 2), Some(vec![0, 0]));
        assert_eq!(decode_hex_exact("0000", 1), None);
    }
}
