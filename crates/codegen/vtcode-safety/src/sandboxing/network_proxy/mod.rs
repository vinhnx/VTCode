//! Local HTTP CONNECT / SOCKS5 proxy used to enforce hostname allowlists.
//!
//! The OS sandbox cannot inspect `connect()` destinations (seccomp BPF sees a
//! pointer, not the sockaddr contents), so exact domain rules are enforced by
//! a loopback proxy: the sandbox permits connections only to the proxy, and
//! the proxy resolves DNS **after** deciding that a hostname is allowed.
//! Deny rules take precedence over allow rules; anything unmatched is denied.

#![expect(
    unused_results,
    reason = "Detached per-connection tasks and byte-copy counts are intentionally discarded."
)]

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tracing::warn;

use crate::sandboxing::NetworkAllowlistEntry;

#[cfg(test)]
mod tests;

const IO_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_HTTP_HEADER: usize = 8 * 1024;

/// Effect of a domain rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainRuleAction {
    Allow,
    Deny,
}

/// One hostname rule. Domains are case-insensitive; `*.example.com` also
/// matches the bare `example.com`, matching the existing allowlist semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainRule {
    pub domain: String,
    pub port: u16,
    pub action: DomainRuleAction,
}

impl DomainRule {
    #[must_use]
    pub fn allow(domain: impl Into<String>, port: u16) -> Self {
        Self {
            domain: domain.into(),
            port,
            action: DomainRuleAction::Allow,
        }
    }

    #[must_use]
    pub fn deny(domain: impl Into<String>, port: u16) -> Self {
        Self {
            domain: domain.into(),
            port,
            action: DomainRuleAction::Deny,
        }
    }

    fn matches(&self, host: &str, port: u16) -> bool {
        if self.port != port {
            return false;
        }
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        let pattern = self.domain.trim_end_matches('.').to_ascii_lowercase();
        if let Some(suffix) = pattern.strip_prefix("*.") {
            host == suffix || host.ends_with(&format!(".{suffix}"))
        } else {
            host == pattern
        }
    }
}

/// Decision for a proxy request. Unknown destinations are denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyDecision {
    Allow,
    Deny,
}

/// Ordered domain rules; deny wins, then allow, then default deny.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomainPolicy {
    rules: Vec<DomainRule>,
}

impl DomainPolicy {
    #[must_use]
    pub fn new(rules: Vec<DomainRule>) -> Self {
        Self { rules }
    }

    /// Build an allow-only policy from the sandbox config's allowlist.
    #[must_use]
    pub fn from_allowlist(entries: &[NetworkAllowlistEntry]) -> Self {
        Self::new(
            entries
                .iter()
                .map(|entry| DomainRule::allow(entry.domain.clone(), entry.port))
                .collect(),
        )
    }

    #[must_use]
    pub fn decide(&self, host: &str, port: u16) -> ProxyDecision {
        if self
            .rules
            .iter()
            .any(|rule| rule.action == DomainRuleAction::Deny && rule.matches(host, port))
        {
            return ProxyDecision::Deny;
        }
        if self
            .rules
            .iter()
            .any(|rule| rule.action == DomainRuleAction::Allow && rule.matches(host, port))
        {
            return ProxyDecision::Allow;
        }
        ProxyDecision::Deny
    }
}

/// Whether this platform can restrict a child to the proxy exactly.
///
/// macOS Seatbelt can allow only the proxy's loopback ports under `(deny
/// default)`. Linux cannot: seccomp cannot inspect `connect()` destinations
/// and this build has no network-namespace relay, so hostname allowlists stay
/// fail-closed there rather than granting unrestricted IP sockets.
#[must_use]
pub const fn proxy_enforcement_available() -> bool {
    cfg!(target_os = "macos")
}

/// Loopback addresses the sandbox must allow outbound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyEndpoints {
    pub http: SocketAddr,
    pub socks5: SocketAddr,
}

impl ProxyEndpoints {
    /// Environment variables that route common HTTP clients through the proxy.
    /// The sandbox's environment allowlist drops unknown variables, so these
    /// are injected after sanitization.
    #[must_use]
    pub fn env_vars(&self) -> Vec<(&'static str, String)> {
        // Use `localhost`, which is also the host the Seatbelt profile permits.
        let http = format!("http://localhost:{}", self.http.port());
        let socks = format!("socks5://localhost:{}", self.socks5.port());
        vec![
            ("HTTP_PROXY", http.clone()),
            ("http_proxy", http.clone()),
            ("HTTPS_PROXY", http.clone()),
            ("https_proxy", http.clone()),
            ("ALL_PROXY", socks.clone()),
            ("all_proxy", socks),
            // Override any inherited bypass list: an unmatched host must go
            // through the policy-enforcing proxy, never direct.
            ("NO_PROXY", String::new()),
            ("no_proxy", String::new()),
            ("PIP_PROXY", http.clone()),
            ("NPM_CONFIG_PROXY", http.clone()),
            ("NPM_CONFIG_HTTPS_PROXY", http.clone()),
            ("CARGO_HTTP_PROXY", http),
        ]
    }
}

/// Running proxy. Dropping it stops both accept loops.
pub struct NetworkProxy {
    endpoints: ProxyEndpoints,
    http_task: JoinHandle<()>,
    socks_task: JoinHandle<()>,
}

impl NetworkProxy {
    #[must_use]
    pub fn endpoints(&self) -> ProxyEndpoints {
        self.endpoints
    }
}

impl Drop for NetworkProxy {
    fn drop(&mut self) {
        self.http_task.abort();
        self.socks_task.abort();
    }
}

/// Start HTTP and SOCKS5 listeners on ephemeral loopback ports.
pub async fn start_proxy(policy: DomainPolicy) -> Result<NetworkProxy> {
    let http_listener = TcpListener::bind(("127.0.0.1", 0)).await.context("bind loopback HTTP proxy")?;
    let socks_listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("bind loopback SOCKS5 proxy")?;
    let endpoints = ProxyEndpoints {
        http: http_listener.local_addr().context("HTTP proxy local address")?,
        socks5: socks_listener.local_addr().context("SOCKS5 proxy local address")?,
    };
    let policy = Arc::new(policy);
    let http_policy = Arc::clone(&policy);
    let http_task = tokio::spawn(async move { serve_http(http_listener, http_policy).await });
    let socks_task = tokio::spawn(async move { serve_socks(socks_listener, policy).await });
    Ok(NetworkProxy { endpoints, http_task, socks_task })
}

async fn serve_http(listener: TcpListener, policy: Arc<DomainPolicy>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let policy = Arc::clone(&policy);
                tokio::spawn(async move {
                    if let Err(error) = handle_http(stream, policy).await {
                        warn!(%error, "HTTP proxy request failed");
                    }
                });
            }
            Err(error) => {
                warn!(%error, "HTTP proxy accept failed");
                return;
            }
        }
    }
}

async fn serve_socks(listener: TcpListener, policy: Arc<DomainPolicy>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let policy = Arc::clone(&policy);
                tokio::spawn(async move {
                    if let Err(error) = handle_socks(stream, policy).await {
                        warn!(%error, "SOCKS5 proxy request failed");
                    }
                });
            }
            Err(error) => {
                warn!(%error, "SOCKS5 proxy accept failed");
                return;
            }
        }
    }
}

async fn handle_http(mut client: TcpStream, policy: Arc<DomainPolicy>) -> Result<()> {
    let mut header = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if header.len() > MAX_HTTP_HEADER {
            bail!("HTTP proxy request header too large");
        }
        let read = timeout(IO_TIMEOUT, client.read(&mut chunk))
            .await
            .context("HTTP proxy read timeout")??;
        if read == 0 {
            return Ok(());
        }
        let bytes = chunk.get(..read).ok_or_else(|| anyhow!("HTTP proxy read exceeded buffer"))?;
        header.extend_from_slice(bytes);
        if header.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let after_request_line = header
        .windows(2)
        .position(|window| window == b"\r\n")
        .ok_or_else(|| anyhow!("HTTP proxy request line missing"))?
        + 2;
    let request_line = std::str::from_utf8(
        header
            .get(..after_request_line.saturating_sub(2))
            .ok_or_else(|| anyhow!("short HTTP header"))?,
    )
    .context("HTTP proxy request line is not UTF-8")?
    .lines()
    .next()
    .unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();

    let (host, port, is_connect) = if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = parse_host_port(target, 443)?;
        (host, port, true)
    } else if target.starts_with("http://") || target.starts_with("https://") {
        let (host, port) = parse_url_authority(target)?;
        (host, port, false)
    } else {
        bail!("HTTP proxy requires CONNECT or an absolute-form URL");
    };

    if policy.decide(&host, port) == ProxyDecision::Deny {
        client
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .context("write HTTP proxy denial")?;
        return Ok(());
    }

    // DNS resolution happens only after the hostname passed the policy check.
    let mut upstream = timeout(IO_TIMEOUT, TcpStream::connect((host.as_str(), port)))
        .await
        .context("HTTP proxy connect timeout")?
        .context("HTTP proxy upstream connect")?;
    if is_connect {
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .context("write CONNECT reply")?;
    } else {
        let origin_form = target
            .strip_prefix("http://")
            .or_else(|| target.strip_prefix("https://"))
            .and_then(|rest| rest.find('/').and_then(|index| rest.get(index..)))
            .unwrap_or("/");
        let rewritten = format!("{method} {origin_form} HTTP/1.1\r\n");
        upstream
            .write_all(rewritten.as_bytes())
            .await
            .context("forward HTTP request line")?;
        upstream
            // Keep the original headers and the terminating blank line; only
            // the request line is rewritten from absolute to origin form.
            .write_all(header.get(after_request_line..).ok_or_else(|| anyhow!("short HTTP header"))?)
            .await
            .context("forward HTTP headers")?;
    }
    copy_bidirectional(&mut client, &mut upstream)
        .await
        .context("relay HTTP proxy traffic")?;
    Ok(())
}

async fn handle_socks(mut client: TcpStream, policy: Arc<DomainPolicy>) -> Result<()> {
    let mut greeting = [0u8; 258];
    let read = timeout(IO_TIMEOUT, client.read(&mut greeting))
        .await
        .context("SOCKS5 greeting timeout")??;
    if read < 2 || greeting[0] != 5 {
        bail!("unsupported SOCKS5 greeting");
    }
    client.write_all(&[5, 0]).await.context("write SOCKS5 greeting reply")?;

    let mut request = [0u8; 4];
    timeout(IO_TIMEOUT, client.read_exact(&mut request))
        .await
        .context("SOCKS5 request timeout")?
        .context("SOCKS5 request header")?;
    if request[0] != 5 || request[1] != 1 {
        client.write_all(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        bail!("unsupported SOCKS5 command");
    }

    let host = match request[3] {
        1 => {
            let mut octets = [0u8; 4];
            client.read_exact(&mut octets).await.context("SOCKS5 IPv4 address")?;
            IpAddr::from(octets).to_string()
        }
        3 => {
            let mut length = [0u8; 1];
            client.read_exact(&mut length).await.context("SOCKS5 domain length")?;
            let mut bytes = vec![0u8; usize::from(length[0])];
            client.read_exact(&mut bytes).await.context("SOCKS5 domain")?;
            String::from_utf8(bytes).context("SOCKS5 domain is not UTF-8")?
        }
        4 => {
            let mut octets = [0u8; 16];
            client.read_exact(&mut octets).await.context("SOCKS5 IPv6 address")?;
            IpAddr::from(octets).to_string()
        }
        _ => bail!("unsupported SOCKS5 address type"),
    };
    let mut port_bytes = [0u8; 2];
    client.read_exact(&mut port_bytes).await.context("SOCKS5 port")?;
    let port = u16::from_be_bytes(port_bytes);

    if policy.decide(&host, port) == ProxyDecision::Deny {
        client
            .write_all(&[5, 2, 0, 1, 0, 0, 0, 0, 0, 0])
            .await
            .context("write SOCKS5 denial")?;
        return Ok(());
    }

    // Do not resolve the hostname until after the policy decision.
    let mut upstream = timeout(IO_TIMEOUT, TcpStream::connect((host.as_str(), port)))
        .await
        .context("SOCKS5 connect timeout")?
        .context("SOCKS5 upstream connect")?;
    client
        .write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0])
        .await
        .context("write SOCKS5 success")?;
    copy_bidirectional(&mut client, &mut upstream)
        .await
        .context("relay SOCKS5 traffic")?;
    Ok(())
}

fn parse_host_port(target: &str, default_port: u16) -> Result<(String, u16)> {
    let host = target.trim().trim_end_matches('/');
    if let Some((host, port)) = host.rsplit_once(':') {
        let port = port.parse::<u16>().context("invalid proxy port")?;
        Ok((host.trim_matches(['[', ']']).to_string(), port))
    } else {
        Ok((host.to_string(), default_port))
    }
}

fn parse_url_authority(target: &str) -> Result<(String, u16)> {
    let rest = target
        .strip_prefix("http://")
        .or_else(|| target.strip_prefix("https://"))
        .ok_or_else(|| anyhow!("unsupported proxy URL scheme"))?;
    let authority = rest.split('/').next().unwrap_or_default();
    let authority = authority.rsplit('@').next().unwrap_or_default();
    let default_port = if target.starts_with("https://") { 443 } else { 80 };
    parse_host_port(authority, default_port)
}
