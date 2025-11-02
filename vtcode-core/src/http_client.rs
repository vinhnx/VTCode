//! Minimal HTTP client shim used for builds/tests. Real implementation lives elsewhere.
use anyhow::Result;
use reqwest::Client;

pub async fn get_text(_url: &str) -> Result<String> {
    // Lightweight placeholder: do not perform network I/O in tests by default.
    Ok(String::new())
}

pub fn default_client() -> Client {
    Client::new()
}
