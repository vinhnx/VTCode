//! HTTP client utilities

use reqwest::{Client, ClientBuilder};
use std::time::Duration;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
pub const SHORT_TIMEOUT: Duration = Duration::from_secs(5);
pub const LONG_TIMEOUT: Duration = Duration::from_secs(300);

fn apply_platform_proxy_policy(builder: ClientBuilder) -> ClientBuilder {
    #[cfg(target_os = "macos")]
    {
        // Avoid system proxy discovery on macOS because it can panic in restricted environments.
        builder.no_proxy()
    }
    #[cfg(not(target_os = "macos"))]
    {
        builder
    }
}

/// Try to build an HTTP client, returning an error if both primary and fallback builders fail.
///
/// This is the fallible version of `build_client`. Use this when you want to handle
/// the error gracefully (e.g., return an error to the caller) instead of panicking.
pub fn try_build_client<F>(configure: F) -> Result<Client, reqwest::Error>
where
    F: Fn(ClientBuilder) -> ClientBuilder,
{
    let primary_builder = configure(apply_platform_proxy_policy(ClientBuilder::new()));
    match primary_builder.build() {
        Ok(client) => Ok(client),
        Err(primary_err) => {
            let fallback_builder = apply_platform_proxy_policy(ClientBuilder::new())
                .timeout(DEFAULT_TIMEOUT)
                .connect_timeout(SHORT_TIMEOUT);
            match fallback_builder.build() {
                Ok(client) => Ok(client),
                Err(fallback_err) => {
                    tracing::error!(
                        primary_error = %primary_err,
                        fallback_error = %fallback_err,
                        "HTTP client creation failed with both primary and fallback configurations"
                    );
                    Err(fallback_err)
                }
            }
        }
    }
}

/// Create a default HTTP client with standard timeouts.
///
/// # Panics
///
/// Panics if the HTTP client cannot be created (e.g., TLS library failure).
/// For a non-panicking version, use [`try_build_client`].
#[allow(clippy::panic)]
fn build_client<F>(configure: F) -> Client
where
    F: Fn(ClientBuilder) -> ClientBuilder,
{
    try_build_client(configure).unwrap_or_else(|e| {
        panic!(
            "failed to build HTTP client: {e}. \
             This usually indicates a TLS configuration issue or system resource exhaustion. \
             Ensure the system has valid TLS certificates and sufficient resources."
        )
    })
}

/// Create a default HTTP client with standard timeouts
pub fn create_default_client() -> Client {
    create_client_with_timeout(DEFAULT_TIMEOUT)
}

/// Create an HTTP client with a custom timeout
pub fn create_client_with_timeout(timeout: Duration) -> Client {
    build_client(|builder| builder.timeout(timeout).connect_timeout(SHORT_TIMEOUT))
}

/// Create an HTTP client with custom connect and request timeouts
pub fn create_client_with_timeouts(connect_timeout: Duration, request_timeout: Duration) -> Client {
    build_client(|builder| {
        builder
            .timeout(request_timeout)
            .connect_timeout(connect_timeout)
    })
}

/// Create an HTTP client with a specific user agent
pub fn create_client_with_user_agent(user_agent: &str) -> Client {
    build_client(|builder| builder.user_agent(user_agent).timeout(DEFAULT_TIMEOUT))
}

/// Create an HTTP client optimized for streaming
pub fn create_streaming_client() -> Client {
    build_client(|builder| {
        builder
            .connect_timeout(SHORT_TIMEOUT)
            .tcp_keepalive(Some(Duration::from_secs(60)))
    })
}

/// Get a default client or create one
pub fn get_or_create_default_client(existing: Option<Client>) -> Client {
    existing.unwrap_or_else(create_default_client)
}
