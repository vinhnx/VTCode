//! WebFetch tool for fetching and analyzing web content using AI
//!
//! Supports both restricted (blocklist) and whitelist (allowlist) modes
//! with dynamic configuration loading from vtcode.toml
//!
//! Fetched content is written to ephemeral temp files under `~/.vtcode/tmp/web_fetch/`
//! and cleaned up periodically. Content is never persisted to the workspace.

use super::traits::Tool;
use crate::config::constants::tools;
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use hashbrown::HashSet;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::IpAddr;
use std::path::PathBuf;
use url::Url;

pub mod classify_helpers;
pub mod domains;
pub use classify_helpers::extract_http_status;
pub use domains::{BUILTIN_BLOCKED_DOMAINS, BUILTIN_BLOCKED_PATTERNS, MALICIOUS_PATTERNS};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_CONTENT_SIZE: usize = 500_000; // 500KB max content size
const MAX_ALLOWED_BYTES: usize = 2_000_000; // 2MB hard cap
const MAX_ALLOWED_TIMEOUT_SECS: u64 = 120; // 2 minutes hard cap

/// Subdirectory under `~/.vtcode/tmp/` for ephemeral web_fetch artifacts.
const TEMP_SUBDIR: &str = "web_fetch";

/// Max age in seconds before temp files are cleaned up (1 hour).
const TEMP_MAX_AGE_SECS: u64 = 3600;

pub(crate) const WEB_FETCH_DESCRIPTION: &str = "Fetches content from a URL and returns an analyzed summary. Accepts: { url: string, prompt?: string, format?: 'summary'|'markdown', max_bytes?: number, timeout_secs?: number }. Set format='markdown' to get the page as cleaned markdown via the defuddle.md extraction service instead of a summary — that service is rate-limited to ONE call per session, so use it sparingly and only for remote http(s) URLs (never local files). Omit prompt for a default summary. For docs domains, try /llms.txt first: for 'abc.com', fetch https://abc.com/llms.txt before the homepage, then traverse linked URLs for relevant Markdown sources. Default max_bytes is 500KB (fits most pages). Do NOT set max_bytes without reason — the default is generous. Truncated responses include truncation metadata so you can retry with a higher budget. Prefer llms.txt over llms-full.txt (can be multi-megabyte). Returns a `temp_file` path to ephemeral fetched content. Read it to analyze. Temp files are auto-cleaned; do not persist elsewhere.";

#[derive(Debug, Deserialize)]
struct WebFetchArgs {
    url: String,
    prompt: String,
    #[serde(default)]
    max_bytes: Option<usize>,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// WebFetch tool that fetches URL content and processes it with AI
#[derive(Clone)]
pub struct WebFetchTool {
    /// Security mode: restricted (blocklist) or whitelist (allowlist)
    pub mode: vtcode_config::WebFetchMode,
    /// Additional blocked domains (merged with builtin)
    pub blocked_domains: HashSet<String>,
    /// Additional blocked patterns (merged with builtin)
    pub blocked_patterns: Vec<String>,
    /// Allowed domains (for exemptions in restricted mode or primary list in whitelist mode)
    pub allowed_domains: HashSet<String>,
    /// Defuddle markdown-extraction backend (used when `format: "markdown"`).
    /// Shared session-cap state survives clones (Arc counter inside).
    pub defuddle: crate::tools::defuddle::DefuddleTool,
    /// Strict HTTPS-only mode
    pub strict_https_only: bool,
}

struct FetchedWebContent {
    content: String,
    truncated_by_max_bytes: bool,
    source_size_bytes: usize,
}

fn fetched_content_from_bytes(bytes: &[u8], max_bytes: usize) -> Result<FetchedWebContent> {
    let source_size_bytes = bytes.len();
    let truncated_by_max_bytes = source_size_bytes > max_bytes;
    if !truncated_by_max_bytes {
        return Ok(FetchedWebContent {
            content: String::from_utf8(bytes.to_vec())
                .context("Response body is not valid UTF-8")?,
            truncated_by_max_bytes,
            source_size_bytes,
        });
    }

    let mut end = max_bytes;

    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }

    let content = std::str::from_utf8(&bytes[..end])
        .context("Response body is not valid UTF-8")?
        .to_string();
    Ok(FetchedWebContent {
        content,
        truncated_by_max_bytes,
        source_size_bytes,
    })
}

/// Returns the path to the ephemeral temp directory for web_fetch artifacts.
/// Creates the directory if it doesn't exist.
async fn web_fetch_temp_dir() -> Result<PathBuf> {
    let base = dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".vtcode")
        .join("tmp")
        .join(TEMP_SUBDIR);
    tokio::fs::create_dir_all(&base)
        .await
        .with_context(|| format!("Failed to create temp directory: {}", base.display()))?;
    Ok(base)
}

/// Write content to an ephemeral temp file and return its path.
/// Files are named with a timestamp for easy age-based cleanup.
async fn write_to_temp_file(content: &str, url: &str) -> Result<PathBuf> {
    let temp_dir = web_fetch_temp_dir().await?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();

    // Create a safe filename from the URL
    let url_hash = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    };

    let filename = format!("{url_hash}_{timestamp}.txt");
    let file_path = temp_dir.join(&filename);

    tokio::fs::write(&file_path, content)
        .await
        .with_context(|| format!("Failed to write temp file: {}", file_path.display()))?;

    Ok(file_path)
}

/// Clean up old web_fetch temp files.
pub async fn cleanup_old_web_fetch_temps(max_age_secs: u64) -> Result<usize> {
    let temp_dir = match web_fetch_temp_dir().await {
        Ok(d) => d,
        Err(_) => return Ok(0),
    };

    if tokio::fs::metadata(&temp_dir).await.is_err() {
        return Ok(0);
    }

    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(max_age_secs))
        .unwrap_or(std::time::UNIX_EPOCH);

    let mut removed = 0;
    let mut entries = match tokio::fs::read_dir(&temp_dir).await {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };

    while let Some(entry) = entries.next_entry().await.unwrap_or(None) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Ok(metadata) = entry.metadata().await
            && let Ok(modified) = metadata.modified()
            && modified <= cutoff
            && tokio::fs::remove_file(&path).await.is_ok()
        {
            removed += 1;
        }
    }

    if removed > 0 {
        tracing::info!(count = removed, "Cleaned up old web_fetch temp files");
    }

    Ok(removed)
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self {
            mode: vtcode_config::WebFetchMode::Restricted,
            blocked_domains: HashSet::new(),
            blocked_patterns: Vec::new(),
            allowed_domains: HashSet::new(),
            defuddle: crate::tools::defuddle::DefuddleTool::new(),
            strict_https_only: true,
        }
    }

    /// Create a WebFetchTool with custom configuration
    pub fn with_config(
        mode: vtcode_config::WebFetchMode,
        blocked_domains: Vec<String>,
        blocked_patterns: Vec<String>,
        allowed_domains: Vec<String>,
        strict_https_only: bool,
    ) -> Self {
        Self {
            mode,
            blocked_domains: blocked_domains.into_iter().collect(),
            blocked_patterns,
            allowed_domains: allowed_domains.into_iter().collect(),
            defuddle: crate::tools::defuddle::DefuddleTool::new(),
            strict_https_only,
        }
    }

    /// Build a `WebFetchTool` from a `WebFetchConfig` value-object. This is
    /// the entry point the tool registry uses so user-configured
    /// allow/block lists and HTTPS settings actually take effect.
    pub fn from_config(config: &vtcode_config::WebFetchConfig) -> Self {
        Self::with_config(
            config.mode,
            config.blocked_domains.clone(),
            config.blocked_patterns.clone(),
            config.allowed_domains.clone(),
            config.strict_https_only,
        )
    }

    async fn fetch_url_content(
        &self,
        url: &str,
        max_bytes: usize,
        timeout_secs: u64,
    ) -> Result<FetchedWebContent> {
        // Validate URL
        self.validate_url(url)?;

        let default_headers = Self::default_headers();

        // Build a redirect policy that validates each redirect target against SSRF rules.
        // Limits to 5 redirects to bound the chain and re-checks security on each hop.
        let blocked_domains = self.blocked_domains.clone();
        let blocked_patterns = self.blocked_patterns.clone();
        let allowed_domains = self.allowed_domains.clone();
        let strict_https = self.strict_https_only;
        let redirect_policy = reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.stop();
            }
            let next_url = attempt.url();
            let next_str = next_url.as_str();

            // Re-validate HTTPS if strict mode is on
            if strict_https && !next_str.starts_with("https://") {
                return attempt.stop();
            }

            // Re-validate the redirect target's domain against the same security checks
            if let Ok(domain) = extract_domain(next_str) {
                if is_private_host(&domain) {
                    return attempt.stop();
                }
                let domain_lower = domain.to_ascii_lowercase();
                if domain_lower.ends_with(".local")
                    || domain_lower.ends_with(".internal")
                    || domain_lower.ends_with(".localhost")
                    || domain_lower.ends_with(".test")
                    || domain_lower.ends_with(".invalid")
                    || domain_lower.ends_with(".home.arpa")
                {
                    return attempt.stop();
                }
                // Check blocked domains
                let mut all_blocked = BUILTIN_BLOCKED_DOMAINS.to_vec();
                all_blocked.extend(blocked_domains.iter().map(|s| s.as_str()));
                for blocked in &all_blocked {
                    if next_str.to_lowercase().contains(blocked) {
                        return attempt.stop();
                    }
                }
                // Check blocked patterns
                let mut all_patterns = BUILTIN_BLOCKED_PATTERNS.to_vec();
                all_patterns.extend(blocked_patterns.iter().map(|s| s.as_str()));
                for pattern in &all_patterns {
                    if next_str.to_lowercase().contains(pattern) {
                        return attempt.stop();
                    }
                }
                // Check allowed domains (exemptions)
                for allowed in &allowed_domains {
                    if domain_matches_allowed(&domain, allowed) {
                        return attempt.follow();
                    }
                }
            }
            attempt.follow()
        });

        let client = reqwest::Client::builder()
            .default_headers(default_headers)
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .redirect(redirect_policy)
            .build()?;

        let response = client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "HTTP request failed with status: {}",
                response.status()
            ));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Validate content type
        self.validate_content_type(&content_type)?;

        // Limit response body to max_bytes while still returning useful partial
        // text. Oversized documentation pages are common, and a truncated fetch
        // gives the agent more signal than an error-only result.
        let bytes = response.bytes().await?;
        fetched_content_from_bytes(&bytes, max_bytes)
    }

    fn validate_url(&self, url: &str) -> Result<()> {
        // HTTPS enforcement (can be disabled only for testing)
        if self.strict_https_only && !url.starts_with("https://") {
            return Err(anyhow!("Only HTTPS URLs are allowed for security"));
        }

        // Parse the URL to extract the real host, which correctly separates
        // userinfo credentials (http://evil@127.0.0.1/) from the host.
        let domain = extract_domain(url)
            .map_err(|e| anyhow!("Failed to parse URL for security validation: {e}"))?;

        // Reject private, loopback, link-local, and reserved IPs.
        if is_private_host(&domain) {
            return Err(anyhow!("Access to local/private networks is blocked"));
        }

        // Block special-use TLDs per RFC 6761 and mDNS/split-DNS conventions:
        // - .local (mDNS)
        // - .internal (private split-DNS)
        // - .localhost (loopback)
        // - .test (testing, RFC 6761)
        // - .invalid (always fails, RFC 6761)
        // - .home.arpa (home networking, RFC 7788)
        let domain_lower = domain.to_ascii_lowercase();
        if domain_lower.ends_with(".local")
            || domain_lower.ends_with(".internal")
            || domain_lower.ends_with(".localhost")
            || domain_lower.ends_with(".test")
            || domain_lower.ends_with(".invalid")
            || domain_lower.ends_with(".home.arpa")
        {
            return Err(anyhow!("Access to local/private networks is blocked"));
        }

        let url_lower = url.to_lowercase();

        // Apply security policy based on mode
        match self.mode {
            vtcode_config::WebFetchMode::Whitelist => self.validate_whitelist_mode(&url_lower)?,
            vtcode_config::WebFetchMode::Restricted => self.validate_restricted_mode(&url_lower)?,
        }

        Ok(())
    }

    fn validate_whitelist_mode(&self, url: &str) -> Result<()> {
        // In whitelist mode, only explicitly allowed domains are permitted
        let domain = extract_domain(url)?;

        if self.allowed_domains.is_empty() {
            return Err(anyhow!(
                "Whitelist mode enabled but no domains are whitelisted. Configure allowed_domains in web_fetch settings."
            ));
        }

        // Check if domain matches any whitelisted domain or pattern
        for allowed in &self.allowed_domains {
            if domain_matches_allowed(&domain, allowed) {
                return Ok(());
            }
        }

        Err(anyhow!(
            "Domain '{domain}' is not in the whitelist. Only explicitly allowed domains are permitted in whitelist mode."
        ))
    }

    fn validate_restricted_mode(&self, url: &str) -> Result<()> {
        // In restricted mode, use a blocklist of known dangerous/sensitive domains
        let url_lower = url.to_lowercase();

        // Check against allowed exemptions first (exemptions override blocklist)
        let domain = extract_domain(url)?;
        for allowed in &self.allowed_domains {
            if domain_matches_allowed(&domain, allowed) {
                return Ok(());
            }
        }

        // Check for malicious and sensitive URL patterns
        self.validate_url_safety(&url_lower)?;

        Ok(())
    }

    fn validate_url_safety(&self, url: &str) -> Result<()> {
        // Combine built-in and custom blocked domains
        let mut all_blocked_domains = BUILTIN_BLOCKED_DOMAINS.to_vec();
        all_blocked_domains.extend(self.blocked_domains.iter().map(|s| s.as_str()));

        // Combine built-in and custom blocked patterns
        let mut all_blocked_patterns = BUILTIN_BLOCKED_PATTERNS.to_vec();
        all_blocked_patterns.extend(self.blocked_patterns.iter().map(|s| s.as_str()));

        // Check blocked domains
        for domain in &all_blocked_domains {
            if url.contains(domain) {
                return Err(anyhow!(
                    "Access to sensitive domain '{domain}' is blocked for privacy and security reasons"
                ));
            }
        }

        // Check for sensitive patterns in URL
        for pattern in &all_blocked_patterns {
            if url.contains(pattern) {
                return Err(anyhow!(
                    "URL contains sensitive pattern '{pattern}'. Fetching URLs with credentials or sensitive data is blocked"
                ));
            }
        }

        // Check for common malware/phishing indicators
        self.check_malicious_indicators(url)?;

        Ok(())
    }

    fn check_malicious_indicators(&self, url: &str) -> Result<()> {
        for pattern in MALICIOUS_PATTERNS {
            if url.contains(pattern) {
                return Err(anyhow!(
                    "URL contains potentially malicious pattern. Access blocked for safety"
                ));
            }
        }

        Ok(())
    }

    fn validate_content_type(&self, content_type: &str) -> Result<()> {
        if content_type.is_empty() {
            return Ok(());
        }

        let allowed_types = [
            "text/html",
            "text/plain",
            "text/markdown",
            "application/json",
            "application/xml",
            "text/xml",
            "application/javascript",
            "text/css",
            "text/javascript",
            "application/xhtml+xml",
        ];

        // Content-Type headers may include parameters (e.g., "text/html; charset=utf-8").
        // Extract just the media type (everything before the first ';') for matching.
        let content_type_lower = content_type.to_lowercase();
        let media_type = content_type_lower
            .split(';')
            .next()
            .unwrap_or(&content_type_lower)
            .trim();

        if allowed_types.contains(&media_type) {
            Ok(())
        } else {
            Err(anyhow!(
                "Content type '{content_type}' is not supported. Only text-based content types are allowed."
            ))
        }
    }

    async fn run(&self, raw_args: Value) -> Result<Value> {
        let args: WebFetchArgs = serde_json::from_value(raw_args).context(
            "Invalid arguments for web_fetch tool. Provide 'url' (and optionally 'prompt').",
        )?;

        let max_bytes = args
            .max_bytes
            .map(|v| v.min(MAX_ALLOWED_BYTES))
            .unwrap_or(MAX_CONTENT_SIZE);
        let timeout_secs = args
            .timeout_secs
            .map(|v| v.min(MAX_ALLOWED_TIMEOUT_SECS))
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        // Fetch the URL content with detailed error handling
        let fetched = match self
            .fetch_url_content(&args.url, max_bytes, timeout_secs)
            .await
        {
            Ok(fetched) => fetched,
            Err(e) => {
                // Categorize the error so the agent loop can react. We used to
                // surface a flat string here, which led to the agent in
                // turn_578 reporting "github.com, npmjs.com, and crates.io are
                // all blocked" when the actual errors were 403/404 from the
                // upstream services.
                return Ok(web_fetch_error_response(
                    &args.url,
                    max_bytes,
                    timeout_secs,
                    &e,
                ));
            }
        };
        let content = fetched.content;

        let content_length = content.len();

        if content_length == 0 {
            return Ok(json!({
                "error": format!(
                    "web_fetch: no content fetched from '{}'. The URL may be unreachable, returned empty content, or used an unsupported content-type.",
                    args.url
                ),
                "url": args.url,
                "error_type": "empty_content",
                "next_action": "The host returned an empty body. This may be a bot block, a JS-only page, or a real empty document. Try web_search to confirm the page exists; if it does, the content is probably JavaScript-rendered and you need a different tool."
            }));
        }

        // Write content to ephemeral temp file instead of keeping it in the response.
        // The temp file is auto-cleaned by age-based cleanup.
        let temp_path = write_to_temp_file(&content, &args.url).await?;
        let temp_path_str = temp_path.to_string_lossy().to_string();

        // Truncate preview for UI display only
        let preview_limit = 8000;
        let (preview, truncated) = if content_length > preview_limit {
            let truncated_content =
                vtcode_commons::formatting::truncate_byte_budget(&content, preview_limit, "...");
            (truncated_content, true)
        } else {
            (content.clone(), false)
        };

        // Cleanup: periodically remove old temp files (every ~50 calls)
        if let Err(e) = cleanup_old_web_fetch_temps(TEMP_MAX_AGE_SECS).await {
            tracing::debug!(error = %e, "Periodic web_fetch temp cleanup failed");
        }

        // Canonical response shape:
        // - `temp_file`: path to ephemeral temp file containing full fetched body
        // - `preview`: truncated snippet (8KB by default) for display
        // - `prompt`: what the user/model wants to know
        // - `next_action_hint`: explicit instruction so the agent continues the loop correctly
        // - `no_spool: true` prevents the output spooler from persisting fetched content to disk
        let mut response = json!({
            "url": args.url,
            "prompt": args.prompt,
            "temp_file": temp_path_str,
            "preview": preview,
            "content_length": content_length,
            "truncated": truncated,
            "no_spool": true,
            "next_action_hint": "Analyze the inline `preview` (the start of the page) using `prompt` and answer the user directly. Only read `temp_file` if you need content beyond the preview; it is ephemeral and may already be cleaned up."
        });

        // Add overflow indicator if preview was truncated
        if truncated {
            response["overflow"] = json!(format!(
                "[+{} more characters]",
                content_length - preview_limit
            ));
        }

        if fetched.truncated_by_max_bytes {
            response["truncated_by_max_bytes"] = json!(true);
            response["max_bytes"] = json!(max_bytes);
            response["source_size_bytes"] = json!(fetched.source_size_bytes);
            response["next_action_hint"] = json!(
                "Read `temp_file` to get the fetched content. Analyze it using `prompt`. If it does not contain enough detail, retry web_fetch with a larger max_bytes or a more specific URL."
            );
        }

        Ok(response)
    }
}

impl WebFetchTool {
    /// Returns default headers used by the WebFetch client. This keeps Accept set to
    /// prefer 'text/markdown' so documentation sites can provide token-efficient markdown
    /// content as a preference.
    fn default_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("text/markdown, */*"));
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("VT Code/1.0 (compatible; web-fetch tool)"),
        );
        headers
    }
}

/// Helper function to extract domain from URL
///
/// Uses proper URL parsing to correctly handle:
/// - User credentials in URLs (`http://user@host/`) — the host is properly
///   separated from the userinfo, preventing SSRF bypass
/// - Port numbers, paths, and query strings
fn extract_domain(url: &str) -> Result<String> {
    let parsed = Url::parse(url).with_context(|| format!("Failed to parse URL: {url}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("URL has no host: {url}"))?;
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
pub(super) fn is_private_host(host: &str) -> bool {
    let trimmed = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    // Try IPv4 / IPv6 parsing first.
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => is_private_ipv4(v4),
            IpAddr::V6(v6) => is_private_ipv6(v6),
        };
    }

    // DNS names like "localhost" that will resolve to loopback.
    if trimmed.eq_ignore_ascii_case("localhost")
        || trimmed.eq_ignore_ascii_case("localhost.localdomain")
    {
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

fn domain_matches_allowed(domain: &str, allowed: &str) -> bool {
    let normalized_domain = domain.trim_end_matches('.').to_ascii_lowercase();
    let normalized_allowed = allowed
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_ascii_lowercase();

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

    normalized_domain == normalized_allowed
        || normalized_domain.ends_with(&format!(".{normalized_allowed}"))
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    async fn execute(&self, mut args: Value) -> Result<Value> {
        // Backwards-compatible argument normalization:
        // - If called with only { "url": "..." } (no prompt), interpret as:
        //   "Fetch this URL and return a concise natural language summary."
        //
        // This ensures:
        // - Simple "fetch https://..." style calls are handled natively by VT Code.
        // - We do not force upstream agents or MCP tools to construct a full prompt.
        // - MCP tools like `get_current_time` remain unaffected (they are separate).
        // `format: "markdown"` routes to the defuddle.md extraction backend
        // (absorbs the former standalone `defuddle_fetch` tool). Session cap
        // and URL validation are enforced by the backend itself.
        if let Some(obj) = args.as_object_mut() {
            let wants_markdown = obj
                .get("format")
                .and_then(Value::as_str)
                .is_some_and(|f| f.eq_ignore_ascii_case("markdown"));
            if wants_markdown {
                let mut defuddle_args = serde_json::Map::new();
                if let Some(url) = obj.get("url").cloned() {
                    defuddle_args.insert("url".to_string(), url);
                }
                if let Some(max_bytes) = obj.get("max_bytes").cloned() {
                    defuddle_args.insert("max_bytes".to_string(), max_bytes);
                }
                return self.defuddle.execute(Value::Object(defuddle_args)).await;
            }
        }

        if let Some(obj) = args.as_object_mut() {
            let has_url = obj.get("url").is_some_and(Value::is_string);
            let has_prompt = obj.get("prompt").is_some_and(Value::is_string);

            if has_url && !has_prompt {
                obj.insert(
                    "prompt".to_string(),
                    json!("Summarize this page concisely. Read the temp_file to get the full content, then focus on the primary purpose, key information, and any actionable details."),
                );
            }
        }

        self.run(args).await
    }

    fn name(&self) -> &str {
        tools::WEB_FETCH
    }

    fn description(&self) -> &str {
        WEB_FETCH_DESCRIPTION
    }
}

/// Build a structured error response for a failed `web_fetch` call.
///
/// The error category (`http_error`, `network_error`, `policy_blocked`,
/// `empty_content`, etc.) plus a `next_action` hint is much more useful to
/// the agent loop than a flat string. The hint is what unblocks the loop
/// in cases like turn_578, where the agent was getting real HTTP 4xx
/// responses but reported the hosts as "blocked" because the error format
/// only carried a free-form message.
fn web_fetch_error_response(
    url: &str,
    max_bytes: usize,
    timeout_secs: u64,
    err: &anyhow::Error,
) -> Value {
    let message = err.to_string();
    let (category, http_status, next_action) = classify_web_fetch_error(&message);

    let mut payload = json!({
        "error": format!("web_fetch: failed to fetch URL '{}': {}", url, message),
        "url": url,
        "max_bytes": max_bytes,
        "timeout_secs": timeout_secs,
        "error_type": category,
        "next_action": next_action,
    });
    if let Some(status) = http_status {
        payload["http_status"] = json!(status);
    }
    payload
}

/// Map a `web_fetch` error string into a stable (category, status,
/// next_action) tuple. The agent loop reads `next_action` to decide whether
/// to retry, fall back to a different tool, or give up.
fn classify_web_fetch_error(message: &str) -> (&'static str, Option<u16>, &'static str) {
    // The reqwest error string is "<METHOD> <URL> error: <chain>". We only
    // look at the error portion because the URL is already in the response.
    let lower = message.to_lowercase();

    // Check timeout / network-class failures BEFORE HTTP status: a slow
    // upstream that returns a 5xx AFTER the body stream stalls can
    // produce a string like "request timed out (last status: 503)".
    // Status-based hints are not the right next-action for that case —
    // the cause is timeout, not server outage.
    if lower.contains("timeout") || lower.contains("timed out") {
        return (
            "network_error",
            None,
            "The request timed out. The host may be slow or unreachable. Try web_search to look up the page title first, or retry with a larger timeout_secs.",
        );
    }
    if lower.contains("dns")
        || lower.contains("name resolution")
        || lower.contains("connection refused")
    {
        return (
            "network_error",
            None,
            "The host could not be reached. Try web_search to look up the page title first; if the host is down, retry later.",
        );
    }
    if let Some(status) = extract_http_status(&lower) {
        return http_status_to_category(status);
    }
    if lower.contains("ssl") || lower.contains("certificate") || lower.contains("tls") {
        return (
            "tls_error",
            None,
            "TLS handshake failed. The host may have an invalid or self-signed certificate. Try web_search as a fallback.",
        );
    }
    if lower.contains("redirect") {
        return (
            "redirect_error",
            None,
            "Redirect chain failed validation. The host may redirect to a blocked domain or loop. Try web_search to find the canonical URL.",
        );
    }
    if lower.contains("unsupported content type") || lower.contains("not supported") {
        return (
            "content_type_error",
            None,
            "The server returned a non-text content type (e.g., image or PDF). The web_fetch tool only handles text/HTML/JSON. Try web_search instead, or use a different tool to read the resource.",
        );
    }
    if lower.contains("blocked") || lower.contains("sensitive") {
        return (
            "policy_blocked",
            None,
            "The URL is on the blocklist for this tool. Use web_search to find an alternative source, or ask the user to whitelist the host via [web_fetch] allowed_domains in vtcode.toml.",
        );
    }
    (
        "unknown_error",
        None,
        "An unexpected error occurred. The error message above is the upstream cause; if it repeats, surface it to the user rather than retrying.",
    )
}

/// Re-export of `classify_helpers::extract_http_status`. Pulled into
/// this module so existing call sites can keep using
/// `extract_http_status(message)` without the extra namespace.
fn http_status_to_category(status: u16) -> (&'static str, Option<u16>, &'static str) {
    match status {
        401 | 407 => (
            "http_error",
            Some(status),
            "The host requires authentication that web_fetch cannot provide. Try web_search for a cached version, or ask the user for credentials.",
        ),
        403 => (
            "http_error",
            Some(status),
            "The host explicitly blocked this request (often anti-bot). Try web_search, or wait a few seconds and retry with a normal browser User-Agent.",
        ),
        404 => (
            "http_error",
            Some(status),
            "The host returned 404: the page or resource does not exist. Verify the URL, or try web_search to find the correct one.",
        ),
        410 => (
            "http_error",
            Some(status),
            "The host returned 410: the resource is permanently gone. Try web_search for an alternative.",
        ),
        429 => (
            "http_error",
            Some(status),
            "The host rate-limited this client. Wait a few seconds and retry, or use web_search instead.",
        ),
        500..=599 => (
            "http_error",
            Some(status),
            "The host returned a server error. Retry after a short delay, or use web_search as a fallback.",
        ),
        _ => (
            "http_error",
            Some(status),
            "The host returned an unexpected status. The error message above is the upstream response; treat it as terminal unless the user asks for a retry.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    async fn execute_json(tool: &WebFetchTool, args: Value) -> Value {
        tool.execute(args)
            .await
            .expect("web_fetch should return structured JSON output")
    }

    fn error_text(result: &Value) -> Option<&str> {
        result.get("error").and_then(Value::as_str)
    }

    #[tokio::test]
    async fn rejects_non_https_urls() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "http://example.com",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("Only HTTPS URLs are allowed"));
    }

    #[tokio::test]
    async fn allows_http_when_https_disabled() {
        let tool = WebFetchTool::with_config(
            vtcode_config::WebFetchMode::Restricted,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false, // strict_https_only = false
        );
        let result = execute_json(
            &tool,
            json!({
                "url": "http://example.com",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        if let Some(error) = error_text(&result) {
            assert!(!error.contains("Only HTTPS URLs are allowed"));
        }
    }

    #[tokio::test]
    async fn rejects_localhost_urls() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://localhost:8080",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("local/private networks"));
    }

    #[tokio::test]
    async fn requires_both_url_and_prompt() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "http://example.com"
            }),
        )
        .await;
        // Prompt should be auto-filled; URL then fails HTTPS policy.
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("Only HTTPS URLs are allowed"));
    }

    #[tokio::test]
    async fn rejects_sensitive_banking_domains() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://paypal.com/login",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("blocked for privacy and security reasons"));
    }

    #[tokio::test]
    async fn rejects_sensitive_auth_domains() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://accounts.google.com",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("blocked for privacy and security reasons"));
    }

    #[tokio::test]
    async fn rejects_urls_with_credentials() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://example.com?password=secret123",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("sensitive pattern"));
    }

    #[tokio::test]
    async fn rejects_urls_with_api_keys() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://api.example.com?api_key=sk_live_123456",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("sensitive pattern"));
    }

    #[tokio::test]
    async fn rejects_urls_with_tokens() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://example.com?token=xyz123",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("sensitive pattern"));
    }

    #[tokio::test]
    async fn rejects_malicious_url_patterns() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://example.com/malware.exe\"",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("potentially malicious pattern"));
    }

    #[tokio::test]
    async fn rejects_typosquatting_domains() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://g00gle.com",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("potentially malicious pattern"));
    }

    #[tokio::test]
    async fn rejects_url_shorteners() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://bit.ly/xyz123",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("potentially malicious pattern"));
    }

    #[tokio::test]
    async fn whitelist_mode_requires_allowed_domains() {
        let tool = WebFetchTool::with_config(
            vtcode_config::WebFetchMode::Whitelist,
            Vec::new(),
            Vec::new(),
            Vec::new(), // No allowed domains
            true,
        );
        let result = execute_json(
            &tool,
            json!({
                "url": "https://example.com",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("").to_string();
        assert!(error.contains("whitelist") || error.contains("whitelisted"));
    }

    #[tokio::test]
    async fn whitelist_mode_allows_whitelisted_domains() {
        let tool = WebFetchTool::with_config(
            vtcode_config::WebFetchMode::Whitelist,
            Vec::new(),
            Vec::new(),
            vec!["example.com".to_string()], // Only example.com allowed
            true,
        );
        let result = execute_json(
            &tool,
            json!({
                "url": "https://example.com/path",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        if let Some(error) = error_text(&result) {
            assert!(!error.contains("not in the whitelist"));
        }
    }

    #[tokio::test]
    async fn whitelist_mode_rejects_non_whitelisted_domains() {
        let tool = WebFetchTool::with_config(
            vtcode_config::WebFetchMode::Whitelist,
            Vec::new(),
            Vec::new(),
            vec!["allowed.com".to_string()],
            true,
        );
        let result = execute_json(
            &tool,
            json!({
                "url": "https://notallowed.com",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("").to_string();
        assert!(error.contains("not in the whitelist"));
    }

    #[tokio::test]
    async fn restricted_mode_allows_exemptions() {
        let tool = WebFetchTool::with_config(
            vtcode_config::WebFetchMode::Restricted,
            Vec::new(),
            Vec::new(),
            vec!["paypal.com".to_string()], // Exempt from blocklist
            true,
        );
        let result = execute_json(
            &tool,
            json!({
                "url": "https://paypal.com/login",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        if let Some(error) = error_text(&result) {
            assert!(!error.contains("blocked for privacy"));
        }
    }

    #[tokio::test]
    async fn custom_blocked_domains_work() {
        let tool = WebFetchTool::with_config(
            vtcode_config::WebFetchMode::Restricted,
            vec!["custom-blocked.com".to_string()], // Custom blocked domain
            Vec::new(),
            Vec::new(),
            true,
        );
        let result = execute_json(
            &tool,
            json!({
                "url": "https://custom-blocked.com/page",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("").to_string();
        assert!(error.contains("blocked for privacy and security reasons"));
    }

    #[tokio::test]
    async fn custom_blocked_patterns_work() {
        let tool = WebFetchTool::with_config(
            vtcode_config::WebFetchMode::Restricted,
            Vec::new(),
            vec!["custom_secret=".to_string()], // Custom pattern
            Vec::new(),
            true,
        );
        let result = execute_json(
            &tool,
            json!({
                "url": "https://example.com?custom_secret=abc123",
                "prompt": "Extract the main content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("").to_string();
        assert!(error.contains("sensitive pattern"));
    }

    #[test]
    fn default_headers_contain_text_markdown_accept() {
        let headers = WebFetchTool::default_headers();
        assert!(headers.contains_key(ACCEPT));
        let val = headers.get(ACCEPT).unwrap().to_str().unwrap();
        assert!(val.contains("text/markdown"));
    }

    #[test]
    fn oversized_body_is_truncated_instead_of_rejected() {
        let fetched = fetched_content_from_bytes("αβγ".as_bytes(), 3)
            .expect("valid utf-8 prefix should be returned");

        assert_eq!(fetched.content, "α");
        assert!(fetched.truncated_by_max_bytes);
        assert_eq!(fetched.source_size_bytes, "αβγ".len());
    }

    #[test]
    fn description_guides_agents_to_try_llms_txt_first() {
        let tool = WebFetchTool::new();
        let description = tool.description();

        assert!(description.contains("/llms.txt"));
        assert!(description.contains("abc.com"));
        assert!(description.contains("https://abc.com/llms.txt"));
        assert!(description.contains("traverse"));
    }

    #[test]
    fn max_bytes_and_timeout_are_clamped_to_hard_caps() {
        const _: () = {
            assert!(MAX_ALLOWED_BYTES >= MAX_CONTENT_SIZE);
            assert!(MAX_ALLOWED_TIMEOUT_SECS >= DEFAULT_TIMEOUT_SECS);
            assert!(MAX_ALLOWED_BYTES <= 10_000_000); // sanity: under 10MB
            assert!(MAX_ALLOWED_TIMEOUT_SECS <= 300); // sanity: under 5 minutes
        };
    }

    #[test]
    fn ipv4_mapped_ipv6_loopback_is_private() {
        // ::ffff:127.0.0.1 should be caught as loopback
        assert!(is_private_host("::ffff:127.0.0.1"));
    }

    #[test]
    fn ipv4_mapped_ipv6_private_is_private() {
        // ::ffff:10.0.0.1 should be caught as private (class A)
        assert!(is_private_host("::ffff:10.0.0.1"));
        // ::ffff:192.168.1.1 should be caught as private (class C)
        assert!(is_private_host("::ffff:192.168.1.1"));
        // ::ffff:172.16.0.1 should be caught as private (class B)
        assert!(is_private_host("::ffff:172.16.0.1"));
    }

    #[test]
    fn ipv4_mapped_ipv6_link_local_is_private() {
        // ::ffff:169.254.0.1 should be caught as link-local
        assert!(is_private_host("::ffff:169.254.0.1"));
    }

    #[test]
    fn ipv4_compatible_ipv6_loopback_is_private() {
        // ::127.0.0.1 (IPv4-compatible) should be caught
        assert!(is_private_host("::127.0.0.1"));
    }

    #[test]
    fn ipv6_loopback_is_private() {
        assert!(is_private_host("::1"));
    }

    #[test]
    fn ipv6_unique_local_is_private() {
        assert!(is_private_host("fd00::1"));
    }

    #[test]
    fn ipv6_link_local_is_private() {
        assert!(is_private_host("fe80::1"));
    }

    #[test]
    fn ipv4_mapped_ipv6_public_is_not_private() {
        // ::ffff:8.8.8.8 (Google DNS) should NOT be blocked
        assert!(!is_private_host("::ffff:8.8.8.8"));
    }

    #[tokio::test]
    async fn rejects_localhost_tld() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://myapp.localhost/api",
                "prompt": "Extract content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("local/private networks"));
    }

    /// Regression test for the "who is vinhnx?" case. The default
    /// `WebFetchConfig` seeds `allowed_domains` with common developer sites
    /// (github.com, npmjs.com, crates.io, etc.). The default `Restricted`
    /// mode honors those exemptions, so URLs on those hosts must pass
    /// `validate_url` without touching the network.
    #[test]
    fn default_restricted_mode_allows_common_dev_sites() {
        let tool = WebFetchTool::from_config(&vtcode_config::WebFetchConfig::default());
        // After H1: the defaults come from the TOML allowlist filtered
        // to web-fetch-relevant categories. `docs.rs` and `www.npmjs.com`
        // are not in the TOML and are no longer in defaults; replace
        // them with registry hosts that ARE in the TOML.
        for url in [
            "https://github.com/vinhnx",
            "https://github.com/vinhnx?tab=repositories",
            "https://api.github.com/users/vinhnx",
            "https://api.github.com/users/vinhnx/repos?sort=stars&per_page=20",
            "https://registry.npmjs.org/vinhnx",
            "https://crates.io/users/vinhnx",
            "https://raw.githubusercontent.com/rust-lang/rust/master/README.md",
            "https://en.wikipedia.org/wiki/Rust_(programming_language)",
            "https://pypi.org/project/requests/",
            "https://r.jina.ai/https://example.com",
        ] {
            tool.validate_url(url)
                .unwrap_or_else(|e| panic!("default restricted mode should allow {url}: {e}"));
        }
    }

    /// Whitelist mode is a strict allowlist: only the user's explicit
    /// `allowed_domains` apply, even if the defaults include the host.
    /// This test constructs a tool with `Whitelist` mode and an empty
    /// user allow list to assert the strict contract.
    #[test]
    fn whitelist_mode_is_strict_when_user_allow_list_is_empty() {
        let tool = WebFetchTool::from_config(&vtcode_config::WebFetchConfig {
            mode: vtcode_config::WebFetchMode::Whitelist,
            allowed_domains: Vec::new(),
            ..vtcode_config::WebFetchConfig::default()
        });
        let err = tool
            .validate_url("https://github.com/vinhnx")
            .expect_err("whitelist mode must reject domains not in the user allow list");
        assert!(
            err.to_string().contains("whitelist"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn domain_matches_allowed_handles_wildcards() {
        // Apex + subdomains under a wildcard.
        assert!(domain_matches_allowed("example.com", "*.example.com"));
        assert!(domain_matches_allowed("api.example.com", "*.example.com"));
        assert!(domain_matches_allowed(
            "deep.nested.api.example.com",
            "*.example.com"
        ));
        // Negative matches — wildcard must not be a substring match.
        assert!(!domain_matches_allowed("evilexample.com", "*.example.com"));
        assert!(!domain_matches_allowed(
            "example.com.evil.tld",
            "*.example.com"
        ));
        // Apex match is also accepted.
        assert!(domain_matches_allowed("example.com", "example.com"));
        assert!(domain_matches_allowed("api.example.com", "example.com"));
    }

    #[test]
    fn domain_matches_allowed_rejects_single_label_wildcards() {
        // L10 (review): a wildcard like `*.com` would match every `.com`
        // host. That's almost certainly a misconfiguration; refuse it
        // outright so an editor who fat-fingers the allowlist doesn't
        // accidentally open the whole TLD.
        assert!(!domain_matches_allowed("example.com", "*.com"));
        assert!(!domain_matches_allowed("api.example.com", "*.com"));
        assert!(!domain_matches_allowed("co.uk", "*.uk"));
        // A multi-label suffix is still allowed.
        assert!(domain_matches_allowed("example.co.uk", "*.co.uk"));
    }

    #[test]
    fn default_allowlist_includes_wildcard_categories() {
        // After H1: the web_fetch defaults are filtered to web-relevant
        // categories. The TOML currently has no `*.foo` wildcards in
        // those categories, so the defaults should not contain any.
        // The matcher itself is still tested for wildcards via
        // `domain_matches_allowed_handles_wildcards` above.
        let config = vtcode_config::WebFetchConfig::default();
        let wildcards: Vec<&str> = config
            .allowed_domains
            .iter()
            .map(|s| s.as_str())
            .filter(|s| s.starts_with("*."))
            .collect();
        assert!(
            wildcards.is_empty(),
            "expected no wildcards in default web_fetch allowlist (TOML has them only in auth/dev_infra which are excluded); got {wildcards:?}"
        );
    }

    #[test]
    fn default_restricted_mode_lets_through_wildcard_hosts() {
        // Regression for the broader TOML allowlist: a URL on a wildcard
        // host should pass `validate_url` under the default `Restricted`
        // mode without any per-session config. The TOML only has wildcards
        // in `auth` / `dev_infra` (excluded by H1), so we exercise the
        // matcher with a user-supplied wildcard instead.
        let tool = WebFetchTool::from_config(&vtcode_config::WebFetchConfig {
            allowed_domains: vec!["*.example.com".to_string()],
            ..vtcode_config::WebFetchConfig::default()
        });
        for url in [
            "https://acme.example.com/authorize",
            "https://deep.nested.api.example.com/api",
        ] {
            tool.validate_url(url)
                .unwrap_or_else(|e| panic!("wildcard allow should accept {url}: {e}"));
        }
    }

    #[tokio::test]
    async fn rejects_test_tld() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://example.test/page",
                "prompt": "Extract content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("local/private networks"));
    }

    #[tokio::test]
    async fn rejects_invalid_tld() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://example.invalid/page",
                "prompt": "Extract content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("local/private networks"));
    }

    #[tokio::test]
    async fn rejects_home_arpa_tld() {
        let tool = WebFetchTool::new();
        let result = execute_json(
            &tool,
            json!({
                "url": "https://myhost.home.arpa/page",
                "prompt": "Extract content"
            }),
        )
        .await;
        let error = error_text(&result).unwrap_or("");
        assert!(error.contains("local/private networks"));
    }

    /// Regression test for turn_578: a real HTTP 403 from npmjs.com
    /// (server-side bot block) must come back as `error_type=http_error`
    /// with `http_status=403`, NOT as a "policy_blocked" or "PermissionDenied"
    /// response. The agent in turn_578 reported the host as "blocked by the
    /// sandbox" because the error format only carried a flat string.
    #[test]
    fn classifies_http_403_as_server_block_not_policy() {
        let err = anyhow::anyhow!("HTTP request failed with status: 403 Forbidden");
        let response = web_fetch_error_response("https://www.npmjs.com/~vinhnx", 262144, 30, &err);
        assert_eq!(response["error_type"], "http_error");
        assert_eq!(response["http_status"], 403);
        let action = response["next_action"].as_str().unwrap_or("");
        assert!(
            action.contains("anti-bot") || action.contains("rate-limited"),
            "next_action should hint at anti-bot / rate-limit, got: {action}"
        );
    }

    /// Regression test for turn_578: a real HTTP 404 from crates.io (the
    /// user does not exist) must come back as `error_type=http_error`
    /// with `http_status=404` and a "the resource does not exist" hint.
    #[test]
    fn classifies_http_404_with_verify_url_hint() {
        let err = anyhow::anyhow!("HTTP request failed with status: 404 Not Found");
        let response = web_fetch_error_response("https://crates.io/users/vinhnx", 262144, 30, &err);
        assert_eq!(response["error_type"], "http_error");
        assert_eq!(response["http_status"], 404);
        let action = response["next_action"].as_str().unwrap_or("");
        assert!(
            action.contains("does not exist"),
            "next_action was: {action}"
        );
    }

    /// 5xx responses are transient — the hint should nudge the agent to
    /// either retry briefly or use web_search as a fallback.
    #[test]
    fn classifies_http_5xx_as_transient() {
        let err = anyhow::anyhow!("HTTP request failed with status: 503 Service Unavailable");
        let response = web_fetch_error_response("https://example.com", 262144, 30, &err);
        assert_eq!(response["error_type"], "http_error");
        assert_eq!(response["http_status"], 503);
        let action = response["next_action"].as_str().unwrap_or("");
        assert!(
            action.contains("retry") || action.contains("search"),
            "got: {action}"
        );
    }

    /// Network-level errors (timeout, DNS, TLS) should be classified
    /// distinctly from HTTP errors so the agent can pick the right
    /// fallback.
    #[test]
    fn classifies_timeout_as_network_error() {
        let err = anyhow::anyhow!("request timed out");
        let response = web_fetch_error_response("https://example.com", 262144, 30, &err);
        assert_eq!(response["error_type"], "network_error");
        assert!(response.get("http_status").is_none() || response["http_status"].is_null());
    }

    #[test]
    fn classifies_tls_error_separately() {
        let err = anyhow::anyhow!("TLS handshake failed: certificate verify failed");
        let response = web_fetch_error_response("https://example.com", 262144, 30, &err);
        assert_eq!(response["error_type"], "tls_error");
    }

    /// The error response must always carry a `next_action` so the agent
    /// loop can decide what to do without re-parsing the free-form message.
    #[test]
    fn every_error_response_has_next_action() {
        let samples = [
            anyhow::anyhow!("HTTP request failed with status: 418 I'm a teapot"),
            anyhow::anyhow!("dns error: no such host"),
            anyhow::anyhow!("something completely unexpected"),
        ];
        for err in &samples {
            let response = web_fetch_error_response("https://example.com", 262144, 30, err);
            let action = response["next_action"].as_str().unwrap_or("");
            assert!(
                !action.is_empty(),
                "next_action must be non-empty; got response {response}"
            );
        }
    }

    #[test]
    fn extract_http_status_delegates_to_classify_helpers() {
        // The local function delegates to `classify_helpers::extract_http_status`,
        // which is the canonical parser. These cases mirror the helpers'
        // own tests so a regression in the delegate is caught here too.
        assert_eq!(extract_http_status("status: 403 Forbidden"), Some(403));
        assert_eq!(
            extract_http_status(
                "HTTP status server error (503 Service Unavailable) for url (https://example.com/)"
            ),
            Some(503)
        );
        assert_eq!(extract_http_status("status:  500 Internal"), Some(500));
        assert_eq!(extract_http_status("no status here"), None);
    }
}
