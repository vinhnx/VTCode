//! HTTP client utilities

use reqwest::{Client, ClientBuilder};
use std::time::Duration;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
pub const SHORT_TIMEOUT: Duration = Duration::from_secs(5);
pub const LONG_TIMEOUT: Duration = Duration::from_secs(300);

/// Create a default HTTP client with standard timeouts
pub fn create_default_client() -> Client {
    create_client_with_timeout(DEFAULT_TIMEOUT)
}

/// Create an HTTP client with a custom timeout
pub fn create_client_with_timeout(timeout: Duration) -> Client {
    ClientBuilder::new()
        .timeout(timeout)
        .connect_timeout(SHORT_TIMEOUT)
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Create an HTTP client with custom connect and request timeouts
pub fn create_client_with_timeouts(connect_timeout: Duration, request_timeout: Duration) -> Client {
    ClientBuilder::new()
        .timeout(request_timeout)
        .connect_timeout(connect_timeout)
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Create an HTTP client with a specific user agent
pub fn create_client_with_user_agent(user_agent: &str) -> Client {
    ClientBuilder::new()
        .user_agent(user_agent)
        .timeout(DEFAULT_TIMEOUT)
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Create an HTTP client optimized for streaming
pub fn create_streaming_client() -> Client {
    ClientBuilder::new()
        .connect_timeout(SHORT_TIMEOUT)
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Get a default client or create one
pub fn get_or_create_default_client(existing: Option<Client>) -> Client {
    existing.unwrap_or_else(create_default_client)
}
