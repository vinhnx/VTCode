//! HTTP client utilities
//!
//! Timeouts here are **defaults, not laws**. A 5-second connect timeout is fine
//! for a warm cache next to the registry and wrong for a first-ever TLS
//! handshake over a slow link; a 30-second request timeout is generous for a
//! metadata fetch and fatal for a cold-start local model. Callers that know
//! better can pass explicit values, and users can override the defaults through
//! `[timeouts] http_connect_seconds` / `http_request_seconds` /
//! `http_download_seconds` (or the `VTCODE_HTTP_*_TIMEOUT_SECS` env vars), which
//! reach this module via [`install_http_timeouts`].

use reqwest::{Client, ClientBuilder};
use std::sync::LazyLock;
use std::time::Duration;

/// Default connect timeout for outbound HTTP clients.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Default whole-request timeout for outbound HTTP clients.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Default timeout for large artifact transfers (release downloads, assets).
const DEFAULT_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// Resolved HTTP timeout defaults shared by every outbound client.
///
/// `None` means "no timeout of this kind" — reqwest then leaves the phase
/// unbounded, which is the documented meaning of a `0` in `[timeouts]`. Keeping
/// the distinction in the type (rather than a `Duration::ZERO` sentinel) means
/// call sites cannot accidentally install a zero deadline that fails instantly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpTimeouts {
    /// Timeout for the TCP/TLS connect phase.
    pub connect: Option<Duration>,
    /// Timeout for the whole request, including the response body.
    pub request: Option<Duration>,
    /// Timeout for large artifact transfers.
    pub download: Option<Duration>,
}

impl Default for HttpTimeouts {
    fn default() -> Self {
        Self {
            connect: Some(DEFAULT_CONNECT_TIMEOUT),
            request: Some(DEFAULT_REQUEST_TIMEOUT),
            download: Some(DEFAULT_DOWNLOAD_TIMEOUT),
        }
    }
}

impl HttpTimeouts {
    /// Resolve a configured number of `[timeouts]` seconds into an optional
    /// duration, where `0` disables the phase.
    #[must_use]
    pub const fn from_seconds(connect: u64, request: u64, download: u64) -> Self {
        Self {
            connect: duration_or_disabled(connect),
            request: duration_or_disabled(request),
            download: duration_or_disabled(download),
        }
    }

    /// Apply the connect and request phases to a builder, skipping disabled
    /// phases instead of installing a zero deadline.
    fn apply_to(self, mut builder: ClientBuilder) -> ClientBuilder {
        if let Some(request) = self.request {
            builder = builder.timeout(request);
        }
        if let Some(connect) = self.connect {
            builder = builder.connect_timeout(connect);
        }
        builder
    }
}

/// `0` means "disabled" throughout `[timeouts]`; keep that convention here so an
/// explicit `0` never turns into an instant-fail `Duration::ZERO` deadline.
const fn duration_or_disabled(seconds: u64) -> Option<Duration> {
    if seconds == 0 {
        None
    } else {
        Some(Duration::from_secs(seconds))
    }
}

static HTTP_TIMEOUTS: LazyLock<arc_swap::ArcSwap<HttpTimeouts>> =
    LazyLock::new(|| arc_swap::ArcSwap::from_pointee(HttpTimeouts::default()));

/// Install process-wide HTTP timeout defaults.
///
/// Called once at startup from the loaded configuration and again on live
/// reload, so `[timeouts]` edits apply without a restart. Clients already built
/// keep the values they were created with; new clients pick these up.
pub fn install_http_timeouts(timeouts: HttpTimeouts) {
    HTTP_TIMEOUTS.store(std::sync::Arc::new(timeouts));
}

/// Current process-wide HTTP timeout defaults.
#[must_use]
pub fn http_timeouts() -> HttpTimeouts {
    **HTTP_TIMEOUTS.load()
}

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
fn try_build_client<F>(configure: F) -> Result<Client, reqwest::Error>
where
    F: Fn(ClientBuilder) -> ClientBuilder,
{
    let primary_builder = configure(apply_platform_proxy_policy(ClientBuilder::new()));
    match primary_builder.build() {
        Ok(client) => Ok(client),
        Err(primary_err) => {
            let fallback_builder = HttpTimeouts::default().apply_to(apply_platform_proxy_policy(ClientBuilder::new()));
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
/// Falls back to a bare default client if the configured builder and its
/// fallback both fail (e.g., TLS misconfiguration, resource exhaustion).
/// The fallback mirrors the resilience pattern in `http_client_pool`: a
/// `tracing::error!` is logged so the failure is observable, but the process
/// does not crash — callers get a working (if unconfigured) client.
fn build_client<F>(configure: F) -> Client
where
    F: Fn(ClientBuilder) -> ClientBuilder,
{
    try_build_client(configure).unwrap_or_else(|e| {
        tracing::error!(
            error = %e,
            "HTTP client build failed (primary + fallback); falling back to bare default client. \
             This usually indicates a TLS configuration issue or system resource exhaustion."
        );
        Client::new()
    })
}

/// Create a default HTTP client with the installed timeout defaults.
pub fn create_default_client() -> Client {
    build_client(|builder| http_timeouts().apply_to(builder))
}

/// Create an HTTP client with a custom request timeout.
///
/// The connect phase keeps the installed default so a stalled TCP/TLS handshake
/// never inherits the (usually much longer) request budget.
pub fn create_client_with_timeout(timeout: Duration) -> Client {
    let connect = http_timeouts().connect;
    build_client(move |builder| {
        let builder = builder.timeout(timeout);
        match connect {
            Some(connect) => builder.connect_timeout(connect),
            None => builder,
        }
    })
}

/// Create an HTTP client with custom connect and request timeouts.
pub fn create_client_with_timeouts(connect_timeout: Duration, request_timeout: Duration) -> Client {
    build_client(|builder| builder.timeout(request_timeout).connect_timeout(connect_timeout))
}

/// Build an origin-bound client without a fallback that could enable redirects.
pub fn create_client_without_redirects(
    connect_timeout: Duration,
    request_timeout: Duration,
) -> Result<Client, reqwest::Error> {
    apply_platform_proxy_policy(ClientBuilder::new())
        .timeout(request_timeout)
        .connect_timeout(connect_timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// Build an origin-bound client without redirects, using the installed defaults.
///
/// For call sites that must not follow redirects (credentials could otherwise be
/// replayed to a new origin) but still need a bounded request.
pub fn create_default_client_without_redirects() -> Result<Client, reqwest::Error> {
    let timeouts = http_timeouts();
    let mut builder = apply_platform_proxy_policy(ClientBuilder::new()).redirect(reqwest::redirect::Policy::none());
    if let Some(request) = timeouts.request {
        builder = builder.timeout(request);
    }
    if let Some(connect) = timeouts.connect {
        builder = builder.connect_timeout(connect);
    }
    builder.build()
}

/// Create an HTTP client with a specific user agent.
pub fn create_client_with_user_agent(user_agent: &str) -> Client {
    let timeouts = http_timeouts();
    build_client(move |mut builder| {
        builder = builder.user_agent(user_agent);
        timeouts.apply_to(builder)
    })
}

/// Create an HTTP client optimized for streaming.
///
/// Streaming intentionally has **no** whole-request timeout: a chat completion
/// that trickles tokens for longer than the request budget is healthy, not hung.
/// The connect phase stays bounded, and callers that need a stall detector should
/// set an idle/`read_timeout` instead of a total deadline.
pub fn create_streaming_client() -> Client {
    let connect = http_timeouts().connect;
    build_client(move |builder| {
        let builder = builder.tcp_keepalive(Some(Duration::from_secs(60)));
        match connect {
            Some(connect) => builder.connect_timeout(connect),
            None => builder,
        }
    })
}

/// Create a client for large artifact transfers (release downloads, assets).
///
/// Uses the installed `download` budget instead of the request budget: a
/// multi-megabyte archive on a cold cache routinely needs longer than the
/// interactive request default, and failing it silently is how installers end up
/// "hanging for 60 seconds and dying" on slower connections.
pub fn create_download_client(user_agent: &str) -> Client {
    let timeouts = http_timeouts();
    build_client(move |mut builder| {
        builder = builder.user_agent(user_agent).tcp_keepalive(Some(Duration::from_secs(60)));
        if let Some(download) = timeouts.download {
            builder = builder.timeout(download);
        }
        if let Some(connect) = timeouts.connect {
            builder = builder.connect_timeout(connect);
        }
        builder
    })
}

/// Get a default client or create one
pub fn get_or_create_default_client(existing: Option<Client>) -> Client {
    existing.unwrap_or_else(create_default_client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeouts_match_historical_constants() {
        let defaults = HttpTimeouts::default();
        assert_eq!(defaults.connect, Some(Duration::from_secs(5)));
        assert_eq!(defaults.request, Some(Duration::from_secs(30)));
        assert_eq!(defaults.download, Some(Duration::from_secs(300)));
    }

    #[test]
    fn zero_seconds_disables_the_phase() {
        let timeouts = HttpTimeouts::from_seconds(0, 0, 0);
        assert_eq!(timeouts.connect, None);
        assert_eq!(timeouts.request, None);
        assert_eq!(timeouts.download, None);
    }

    #[test]
    fn from_seconds_preserves_explicit_values() {
        let timeouts = HttpTimeouts::from_seconds(12, 45, 900);
        assert_eq!(timeouts.connect, Some(Duration::from_secs(12)));
        assert_eq!(timeouts.request, Some(Duration::from_secs(45)));
        assert_eq!(timeouts.download, Some(Duration::from_secs(900)));
    }

    /// Restores the process-global defaults on drop so a failing assertion
    /// cannot leak installed timeouts into sibling tests.
    struct InstalledTimeoutsGuard(HttpTimeouts);

    impl InstalledTimeoutsGuard {
        fn recording_current() -> Self {
            Self(http_timeouts())
        }
    }

    impl Drop for InstalledTimeoutsGuard {
        fn drop(&mut self) {
            install_http_timeouts(self.0);
        }
    }

    // These two tests mutate process-global state; `serial` keeps them from
    // interleaving with each other under nextest's parallel threads.
    #[test]
    #[serial_test::serial]
    fn installing_timeouts_is_observable_and_restorable() {
        let _guard = InstalledTimeoutsGuard::recording_current();

        install_http_timeouts(HttpTimeouts::from_seconds(0, 7, 11));
        assert_eq!(http_timeouts().connect, None);
        assert_eq!(http_timeouts().request, Some(Duration::from_secs(7)));
        assert_eq!(http_timeouts().download, Some(Duration::from_secs(11)));
    }

    #[test]
    #[serial_test::serial]
    fn clients_build_for_every_timeout_shape() {
        let _guard = InstalledTimeoutsGuard::recording_current();

        // Disabled phases must not panic or install a zero deadline.
        for timeouts in [
            HttpTimeouts::default(),
            HttpTimeouts::from_seconds(0, 0, 0),
            HttpTimeouts::from_seconds(1, 1, 1),
        ] {
            install_http_timeouts(timeouts);
            drop(create_default_client());
            drop(create_client_with_timeout(Duration::from_secs(3)));
            drop(create_streaming_client());
            drop(create_client_with_user_agent("vtcode-test"));
            drop(create_download_client("vtcode-test"));
            let origin_bound =
                create_default_client_without_redirects().expect("client without redirects should build");
            drop(origin_bound);
        }
    }
}
