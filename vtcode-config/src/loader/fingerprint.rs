use sha2::{Digest, Sha256};
use toml::Value as TomlValue;

pub fn fingerprint_toml_value(value: &TomlValue) -> String {
    let serialized = toml::to_string(value).unwrap_or_else(|_| value.to_string());
    fingerprint_str(&serialized)
}

pub fn fingerprint_str(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}
