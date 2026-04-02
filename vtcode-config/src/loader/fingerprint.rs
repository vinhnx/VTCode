use sha2::{Digest, Sha256};
use toml::Value as TomlValue;

pub fn fingerprint_toml_value(value: &TomlValue) -> String {
    let serialized = toml::to_string(value).unwrap_or_else(|_| value.to_string());
    fingerprint_str(&serialized)
}

pub fn fingerprint_str(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let mut output = String::with_capacity(digest.len() * 2);

    for byte in digest {
        output.push(nibble_to_hex(byte >> 4));
        output.push(nibble_to_hex(byte & 0x0f));
    }

    output
}

fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + (nibble - 10)),
        _ => unreachable!("nibble must be in 0..=15"),
    }
}
