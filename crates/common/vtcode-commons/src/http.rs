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
//!
//! This module is the home of the timeout policy, so it is the one place that
//! may build a bare `ClientBuilder`. Every other crate must go through the
//! helpers below so the installed budgets apply.

#![allow(
    clippy::disallowed_methods,
    reason = "This module implements the shared timeout policy; bare builders are the policy's own building blocks."
)]

use reqwest::{Client, ClientBuilder};
use std::future::Future;
use std::sync::LazyLock;
use std::time::Duration;

use crate::retry::RetryPolicy;

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

/// Apply the installed connect budget to an explicit client builder.
///
/// Prefer the `create_*` constructors; use this only where a call site needs
/// builder options they do not offer (redirect or retry policy tweaks).
/// A `None` budget (the phase was disabled) leaves the builder untouched.
pub fn apply_connect_timeout(builder: ClientBuilder) -> ClientBuilder {
    match http_timeouts().connect {
        Some(connect) => builder.connect_timeout(connect),
        None => builder,
    }
}

/// Send one request per attempt with capped exponential backoff and jitter.
///
/// The caller rebuilds the request on every attempt via `make_request` (a
/// `RequestBuilder` is consumed by `send()`, so it cannot be reused).
/// Transport failures (connect errors, timeouts) are retried; builder errors
/// fail fast because retrying a malformed request can never succeed.
/// Responses with a retryable status (429/500/502/503/504 per
/// [`RetryPolicy::classify_status`]) are retried while attempts remain;
/// otherwise the final response is returned for the caller to handle.
///
/// This is the shared helper for idempotent background fetches (release
/// metadata, asset downloads, catalog refreshes). Interactive LLM requests
/// must NOT use it: the runloop already retries those under its own budget,
/// and stacking a transport retry underneath would multiply dispatches and
/// provider billing.
///
/// What this does NOT do: it never honors `Retry-After` (policy backoff
/// only; `Retry-After` cooperation stays at the tool-registry layer where the
/// server relationship exists), and it never retries a response body failure —
/// only the initial `send()`.
pub async fn send_with_retry<F, Fut>(policy: &RetryPolicy, make_request: F) -> Result<reqwest::Response, reqwest::Error>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<reqwest::Response, reqwest::Error>>,
{
    send_with_retry_inner(policy, true, make_request).await
}

/// Retry only when no response was received (transport failure), never on an
/// HTTP status. For non-idempotent OAuth token POSTs: an authorization code
/// may already be consumed when a 5xx arrives, so a status-triggered retry
/// would just burn the login; a transport error means the server likely never
/// saw the request, making one more attempt strictly better than failing.
pub async fn send_with_retry_transport_only<F, Fut>(
    policy: &RetryPolicy,
    make_request: F,
) -> Result<reqwest::Response, reqwest::Error>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<reqwest::Response, reqwest::Error>>,
{
    send_with_retry_inner(policy, false, make_request).await
}

async fn send_with_retry_inner<F, Fut>(
    policy: &RetryPolicy,
    retry_status: bool,
    make_request: F,
) -> Result<reqwest::Response, reqwest::Error>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<reqwest::Response, reqwest::Error>>,
{
    // Termination variant: `attempt` rises toward `attempts`, and the `last`
    // arm returns on the final iteration, so the loop always exits.
    let attempts = policy.max_attempts.max(1);
    let mut attempt: u32 = 0;
    loop {
        if attempt > 0 {
            tokio::time::sleep(policy.delay_for_attempt(attempt - 1)).await;
        }
        let last = attempt + 1 >= attempts;
        match make_request().await {
            Ok(response) if !retry_status || last || !policy.classify_status(response.status().as_u16()).retryable => {
                return Ok(response);
            }
            Ok(_) => {}
            Err(error) if error.is_builder() || last => return Err(error),
            Err(_) => {}
        }
        attempt += 1;
    }
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

    /// Reserved, unroutable TEST-NET-2 address (RFC 5737): connection attempts
    /// stall in (or fail fast from) the handshake without any live server.
    const UNROUTABLE_URL: &str = "http://198.51.100.1:9/hang";

    /// Outer bound for every stall test: the client budget must fire long
    /// before this guard, which exists only to fail (never hang) the suite
    /// if a timeout regresses.
    const STALL_TEST_OUTER_GUARD: Duration = Duration::from_secs(15);

    /// Bind an ephemeral localhost fixture listener. Returns `None` when the
    /// sandbox forbids binding (callers skip): mirrors the OAuth
    /// callback-server tests, which cannot assume socket access either.
    fn bind_fixture_listener() -> Option<std::net::TcpListener> {
        match std::net::TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => Some(listener),
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => None,
            Err(err) => panic!("bind fixture listener: {err}"),
        }
    }

    /// Fixture for request-budget tests: a bound listener that is never
    /// accepted, so the kernel completes the TCP handshake via the backlog
    /// and the request stalls waiting for a response. The listener must stay
    /// alive for the whole request.
    fn stalled_listener() -> Option<std::net::TcpListener> {
        bind_fixture_listener()
    }

    /// Serve exactly one minimal `200 OK` on a background thread, then exit.
    /// The handle is deliberately detached, never joined: joining would hang
    /// the suite if the client errored before sending.
    fn respond_once(listener: std::net::TcpListener) {
        drop(std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test connection");
            use std::io::{Read, Write};
            let mut request = [0u8; 4096];
            let request_bytes = stream.read(&mut request).expect("read test request");
            assert!(request_bytes > 0, "client must send the request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .expect("write test response");
        }));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn default_client_request_budget_bounds_an_accept_then_stall_server() {
        let _guard = InstalledTimeoutsGuard::recording_current();
        install_http_timeouts(HttpTimeouts::from_seconds(5, 2, 300));

        let Some(listener) = stalled_listener() else { return };
        let addr = listener.local_addr().expect("listener address");

        let client = create_default_client();
        let started = std::time::Instant::now();
        let outcome =
            tokio::time::timeout(STALL_TEST_OUTER_GUARD, client.get(format!("http://{addr}/hang")).send()).await;
        let elapsed = started.elapsed();

        let error = outcome
            .expect("request budget must fire before the outer guard")
            .expect_err("a stalled server must not succeed");
        assert!(error.is_timeout(), "expected a timeout error, got: {error}");
        assert!(elapsed >= Duration::from_secs(2), "2s budget returned after {elapsed:?}");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn default_client_connect_phase_cannot_hang_on_unroutable_host() {
        let _guard = InstalledTimeoutsGuard::recording_current();
        install_http_timeouts(HttpTimeouts::from_seconds(2, 30, 300));

        let client = create_default_client();
        let outcome = tokio::time::timeout(STALL_TEST_OUTER_GUARD, client.get(UNROUTABLE_URL).send()).await;

        // No `is_timeout`/elapsed asserts here: some environments fail fast
        // with "unreachable" instead of timing out. The guarantee under test
        // is that the connect phase always resolves bounded with a diagnostic.
        let error = outcome
            .expect("connect phase must resolve before the outer guard")
            .expect_err("an unroutable endpoint must not succeed");
        assert!(!error.to_string().is_empty(), "failure must carry a diagnostic");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn origin_bound_client_request_budget_bounds_a_stalled_server() {
        let _guard = InstalledTimeoutsGuard::recording_current();
        install_http_timeouts(HttpTimeouts::from_seconds(5, 2, 300));

        let Some(listener) = stalled_listener() else { return };
        let addr = listener.local_addr().expect("listener address");

        let client = create_default_client_without_redirects().expect("origin-bound client builds");
        let started = std::time::Instant::now();
        let outcome =
            tokio::time::timeout(STALL_TEST_OUTER_GUARD, client.get(format!("http://{addr}/hang")).send()).await;
        let elapsed = started.elapsed();

        let error = outcome
            .expect("request budget must fire before the outer guard")
            .expect_err("a stalled server must not succeed");
        assert!(error.is_timeout(), "expected a timeout error, got: {error}");
        assert!(elapsed >= Duration::from_secs(2), "2s budget returned after {elapsed:?}");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn bounded_clients_still_succeed_against_a_healthy_server() {
        let _guard = InstalledTimeoutsGuard::recording_current();
        install_http_timeouts(HttpTimeouts::from_seconds(5, 5, 300));

        let Some(listener) = bind_fixture_listener() else {
            return;
        };
        let addr = listener.local_addr().expect("listener address");
        respond_once(listener);

        let outcome =
            tokio::time::timeout(STALL_TEST_OUTER_GUARD, create_default_client().get(format!("http://{addr}/")).send())
                .await;
        let body = outcome
            .expect("healthy server must answer before the outer guard")
            .expect("bounded client must succeed")
            .text()
            .await
            .expect("read healthy body");
        assert_eq!(body, "ok");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn apply_connect_timeout_bounds_an_explicit_builder() {
        let _guard = InstalledTimeoutsGuard::recording_current();
        install_http_timeouts(HttpTimeouts::from_seconds(2, 30, 300));

        let client = apply_connect_timeout(Client::builder())
            .build()
            .expect("explicit builder builds");
        let outcome = tokio::time::timeout(STALL_TEST_OUTER_GUARD, client.get(UNROUTABLE_URL).send()).await;

        // Same robustness caveat as the unroutable-host test: the guarantee
        // under test is a bounded connect phase with a diagnostic.
        let error = outcome
            .expect("connect phase must resolve before the outer guard")
            .expect_err("an unroutable endpoint must not succeed");
        assert!(!error.to_string().is_empty(), "failure must carry a diagnostic");
    }

    /// Serve a scripted sequence of statuses on a background thread, one per
    /// connection. Detached like [`respond_once`]: never joined.
    fn respond_scripted(listener: std::net::TcpListener, statuses: Vec<u16>) {
        drop(std::thread::spawn(move || {
            use std::io::{Read, Write};
            for status in statuses {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut request = [0u8; 4096];
                if stream.read(&mut request).is_err() {
                    break;
                }
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
                if stream.write_all(response.as_bytes()).is_err() {
                    break;
                }
            }
        }));
    }

    /// Reserve an ephemeral port and release it, so connections are refused
    /// fast. Returns `None` when the sandbox forbids binding (callers skip).
    fn reserve_refused_port() -> Option<u16> {
        match std::net::TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => Some(listener.local_addr().expect("listener address").port()),
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => None,
            Err(err) => panic!("reserve refused port: {err}"),
        }
    }

    fn retry_test_policy() -> RetryPolicy {
        RetryPolicy::from_retries(2, Duration::from_millis(1), Duration::from_millis(5), 2.0)
            .with_jitter(RetryPolicy::DEFAULT_JITTER)
    }

    fn retry_test_client() -> Client {
        create_client_with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
    }

    #[tokio::test]
    async fn retryable_status_is_retried_until_success() {
        let Some(listener) = bind_fixture_listener() else {
            return;
        };
        let addr = listener.local_addr().expect("listener address");
        respond_scripted(listener, vec![503, 429, 200]);

        let client = retry_test_client();
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let policy = retry_test_policy();
        let outcome = tokio::time::timeout(
            STALL_TEST_OUTER_GUARD,
            send_with_retry(&policy, || {
                let _ = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                client.get(format!("http://{addr}/")).send()
            }),
        )
        .await
        .expect("scripted server must answer before the outer guard")
        .expect("503,429,200 must recover");

        assert_eq!(outcome.status().as_u16(), 200);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn exhausted_status_budget_returns_the_final_response() {
        let Some(listener) = bind_fixture_listener() else {
            return;
        };
        let addr = listener.local_addr().expect("listener address");
        respond_scripted(listener, vec![503, 503, 503, 503]);

        let client = retry_test_client();
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let policy = retry_test_policy();
        let outcome = tokio::time::timeout(
            STALL_TEST_OUTER_GUARD,
            send_with_retry(&policy, || {
                let _ = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                client.get(format!("http://{addr}/")).send()
            }),
        )
        .await
        .expect("scripted server must answer before the outer guard")
        .expect("exhaustion returns the response, not an error");

        // The helper reports; the caller decides: a terminal 503 arrives as a
        // response for `error_for_status` handling, not a transport error.
        assert_eq!(outcome.status().as_u16(), 503);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn non_retryable_status_returns_immediately() {
        let Some(listener) = bind_fixture_listener() else {
            return;
        };
        let addr = listener.local_addr().expect("listener address");
        respond_scripted(listener, vec![404, 200]);

        let client = retry_test_client();
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let policy = retry_test_policy();
        let outcome = tokio::time::timeout(
            STALL_TEST_OUTER_GUARD,
            send_with_retry(&policy, || {
                let _ = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                client.get(format!("http://{addr}/")).send()
            }),
        )
        .await
        .expect("scripted server must answer before the outer guard")
        .expect("404 must arrive as a response");

        // Asymmetric partner to the 503/429 recovery: 404 never retries, so the
        // scripted 200 is never consumed.
        assert_eq!(outcome.status().as_u16(), 404);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn transport_failure_is_retried_then_returned() {
        let Some(port) = reserve_refused_port() else {
            return;
        };

        let client = retry_test_client();
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let policy = retry_test_policy();
        let outcome = tokio::time::timeout(
            STALL_TEST_OUTER_GUARD,
            send_with_retry(&policy, || {
                let _ = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                client.get(format!("http://127.0.0.1:{port}/")).send()
            }),
        )
        .await
        .expect("refused connections must resolve before the outer guard");

        assert!(outcome.is_err(), "a refused port must not succeed");
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn builder_error_fails_fast_without_retry() {
        let client = retry_test_client();
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let policy = retry_test_policy();
        let outcome = send_with_retry(&policy, || {
            let _ = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            client.get("http://exa mple.invalid/").send()
        })
        .await;

        // A malformed request can never succeed on retry: one attempt, and the
        // error must be recognizable as a construction failure.
        let error = outcome.expect_err("a malformed URL must not succeed");
        assert!(error.is_builder(), "expected a builder error, got: {error}");
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn transport_only_helper_ignores_retryable_status() {
        let Some(listener) = bind_fixture_listener() else {
            return;
        };
        let addr = listener.local_addr().expect("listener address");
        respond_scripted(listener, vec![503, 200]);

        let client = retry_test_client();
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let policy = RetryPolicy::from_retries(1, Duration::from_millis(1), Duration::from_millis(5), 2.0)
            .with_jitter(RetryPolicy::DEFAULT_JITTER);
        let outcome = tokio::time::timeout(
            STALL_TEST_OUTER_GUARD,
            send_with_retry_transport_only(&policy, || {
                let _ = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                client.get(format!("http://{addr}/")).send()
            }),
        )
        .await
        .expect("scripted server must answer before the outer guard")
        .expect("503 must arrive as a response");

        assert_eq!(outcome.status().as_u16(), 503);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn transport_only_helper_still_retries_transport_failure() {
        let Some(port) = reserve_refused_port() else {
            return;
        };

        let client = retry_test_client();
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let policy = RetryPolicy::from_retries(1, Duration::from_millis(1), Duration::from_millis(5), 2.0)
            .with_jitter(RetryPolicy::DEFAULT_JITTER);
        let outcome = tokio::time::timeout(
            STALL_TEST_OUTER_GUARD,
            send_with_retry_transport_only(&policy, || {
                let _ = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                client.get(format!("http://127.0.0.1:{port}/")).send()
            }),
        )
        .await
        .expect("refused connections must resolve before the outer guard");

        assert!(outcome.is_err(), "a refused port must not succeed");
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
