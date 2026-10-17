//! SSRF network-guard helpers for `web_fetch`: domain extraction, private-host detection, and allowlist matching.

use anyhow::{Context, Result, anyhow, bail};
use std::net::IpAddr;
use url::Url;

/// Helper function to extract domain from URL
///
/// Uses proper URL parsing to correctly handle:
/// - User credentials in URLs (`http://user@host/`) — the host is properly
///   separated from the userinfo, preventing SSRF bypass
/// - Port numbers, paths, and query strings
pub(super) fn extract_domain(url: &str) -> Result<String> {
    let parsed = Url::parse(url).with_context(|| format!("Failed to parse URL: {url}"))?;
    let host = parsed.host_str().ok_or_else(|| anyhow!("URL has no host: {url}"))?;
    if host.is_empty() {
        bail!("URL has empty host: {url}");
    }
    Ok(host.to_string())
}

/// Returns `true` when `host` is a private, loopback, or link-local IP address.
/// Return `true` when `host` is a loopback, private, link-local,
/// broadcast, or multicast address. Used by the SSRF guard in
/// `web_fetch::validate_url` and re-used by `defuddle` so a private host
/// can never sneak through the defuddle relay.
///
/// Accepts the host either bare or bracket-wrapped (the URL parser
/// returns IPv6 hosts like `[::1]`; we strip the brackets before
/// parsing).
pub(crate) fn is_private_host(host: &str) -> bool {
    let trimmed = host.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(host);
    // Try IPv4 / IPv6 parsing first.
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => is_private_ipv4(v4),
            IpAddr::V6(v6) => is_private_ipv6(v6),
        };
    }

    // DNS names like "localhost" that will resolve to loopback.
    if trimmed.eq_ignore_ascii_case("localhost") || trimmed.eq_ignore_ascii_case("localhost.localdomain") {
        return true;
    }

    false
}

/// Check if an IPv4 address is private, loopback, link-local, broadcast,
/// or multicast.
fn is_private_ipv4(v4: std::net::Ipv4Addr) -> bool {
    let octets = v4.octets();
    // 127.0.0.0/8 — loopback (is_loopback only matches 127.0.0.1)
    octets[0] == 127
        // 10.0.0.0/8 — class A private
        || octets[0] == 10
        // 172.16.0.0/12 — class B private
        || (octets[0] == 172 && (octets[1] & 0xf0) == 16)
        // 192.168.0.0/16 — class C private
        || (octets[0] == 192 && octets[1] == 168)
        // 169.254.0.0/16 — link-local
        || (octets[0] == 169 && octets[1] == 254)
        // 0.0.0.0/8 — "this network" (covers 0.0.0.0 itself; is_unspecified
        // would only match 0.0.0.0 strictly)
        || octets[0] == 0
        // 255.255.255.255 — broadcast
        || v4.is_broadcast()
        // 224.0.0.0/4 — multicast
        || (octets[0] & 0xf0) == 224
}

/// Check if an IPv6 address is private, loopback, link-local, or an
/// IPv4-mapped/compatible address pointing to a private IPv4 range.
fn is_private_ipv6(v6: std::net::Ipv6Addr) -> bool {
    let segments = v6.segments();

    // Direct IPv6 checks
    if v6.is_loopback()
        || v6.is_unspecified()
        // fc00::/7 — unique local unicast
        || (segments[0] & 0xfe00) == 0xfc00
        // fe80::/10 — link-local unicast
        || (segments[0] & 0xffc0) == 0xfe80
    {
        return true;
    }

    // Check for IPv4-mapped (::ffff:a.b.c.d) and IPv4-compatible (::a.b.c.d) addresses.
    // These embed an IPv4 address in the low 32 bits; the embedded IPv4 must also be checked.
    // Format: segments[0..4] are 0x0000 or 0xffff, segments[4..6] are 0x0000,
    // then segments[6..8] hold the IPv4 address.
    let is_ipv4_mapped = segments[0] == 0
        && segments[1] == 0
        && segments[2] == 0
        && segments[3] == 0
        && segments[4] == 0
        && segments[5] == 0xffff;
    let is_ipv4_compat = segments[0] == 0
        && segments[1] == 0
        && segments[2] == 0
        && segments[3] == 0
        && segments[4] == 0
        && segments[5] == 0;

    if is_ipv4_mapped || is_ipv4_compat {
        let embedded = std::net::Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            (segments[6] & 0xff) as u8,
            (segments[7] >> 8) as u8,
            (segments[7] & 0xff) as u8,
        );
        return is_private_ipv4(embedded);
    }

    false
}

pub(super) fn domain_matches_allowed(domain: &str, allowed: &str) -> bool {
    let normalized_domain = domain.trim_end_matches('.').to_ascii_lowercase();
    let normalized_allowed = allowed.trim_start_matches('.').trim_end_matches('.').to_ascii_lowercase();

    // Wildcard entries like `*.example.com` match the apex and any
    // subdomain. The `*` must be a complete leading label, not a partial
    // match (so `*.example.com` matches `api.example.com` but not
    // `evilexample.com`).
    if let Some(suffix) = normalized_allowed.strip_prefix("*.") {
        // Reject single-label wildcards like `*.com` — that would match
        // every `.com` host, which is almost certainly a misconfiguration
        // and a serious over-grant. The suffix must contain at least one
        // dot, e.g. `*.example.com` or `*.co.uk`.
        if !suffix.contains('.') {
            return false;
        }
        return normalized_domain == suffix || normalized_domain.ends_with(&format!(".{suffix}"));
    }

    normalized_domain == normalized_allowed || normalized_domain.ends_with(&format!(".{normalized_allowed}"))
}
