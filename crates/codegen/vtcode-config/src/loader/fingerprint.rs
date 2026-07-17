use toml::Value as TomlValue;
use vtcode_commons::utils::calculate_sha256;

pub fn fingerprint_toml_value(value: &TomlValue) -> String {
    let serialized = toml::to_string(value).unwrap_or_else(|_| value.to_string());
    fingerprint_str(&serialized)
}

pub fn fingerprint_str(value: &str) -> String {
    calculate_sha256(value.as_bytes())
}
