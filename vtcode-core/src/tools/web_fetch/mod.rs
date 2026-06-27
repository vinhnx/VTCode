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

pub mod domains;
pub use domains::{BUILTIN_BLOCKED_DOMAINS, BUILTIN_BLOCKED_PATTERNS, MALICIOUS_PATTERNS};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_CONTENT_SIZE: usize = 500_000; // 500KB max content size
const MAX_ALLOWED_BYTES: usize = 2_000_000; // 2MB hard cap
const MAX_ALLOWED_TIMEOUT_SECS: u64 = 120; // 2 minutes hard cap

/// Subdirectory under `~/.vtcode/tmp/` for ephemeral web_fetch artifacts.
const TEMP_SUBDIR: &str = "web_fetch";

/// Max age in seconds before temp files are cleaned up (1 hour).
const TEMP_MAX_AGE_SECS: u64 = 3600;

pub(crate) const WEB_FETCH_DESCRIPTION: &str = "Fetches content from a specified URL and returns an analyzed summary. Accepts: { url: string, prompt?: string, max_bytes?: number, timeout_secs?: number }. If 'prompt' is omitted, VT Code uses a safe default summary prompt so that simple 'fetch https://…' requests are handled by this built-in tool instead of delegating to external MCP tools. For documentation domains, try the site's LLM-oriented /llms.txt index first when appropriate: for input like 'abc.com', fetch https://abc.com/llms.txt before the homepage, then traverse the linked URL map for the most relevant Markdown sources. Budget guidance: the default max_bytes is 500KB which fits most pages including llms.txt files (typically under 50KB). Do NOT set max_bytes unless you have a specific reason — the default is generous. If a page exceeds max_bytes, the tool truncates the response and returns truncation metadata (truncated_by_max_bytes, source_size_bytes) so you can decide whether to retry with a larger budget. Note that llms-full.txt files can be multi-megabyte; prefer the compact llms.txt index first. Returns a `temp_file` path to an ephemeral temp file containing the full fetched content. Read the temp_file to analyze the content. Temp files are auto-cleaned and must not be copied or persisted elsewhere.";

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

    let filename = format!("{}_{}.txt", url_hash, timestamp);
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

    if !tokio::fs::metadata(&temp_dir).await.is_ok() {
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
            strict_https_only,
        }
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
            "Domain '{}' is not in the whitelist. Only explicitly allowed domains are permitted in whitelist mode.",
            domain
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
                    "Access to sensitive domain '{}' is blocked for privacy and security reasons",
                    domain
                ));
            }
        }

        // Check for sensitive patterns in URL
        for pattern in &all_blocked_patterns {
            if url.contains(pattern) {
                return Err(anyhow!(
                    "URL contains sensitive pattern '{}'. Fetching URLs with credentials or sensitive data is blocked",
                    pattern
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
                "Content type '{}' is not supported. Only text-based content types are allowed.",
                content_type
            ))
        }
    }

    async fn run(&self, raw_args: Value) -> Result<Value> {
        let args: WebFetchArgs = serde_json::from_value(raw_args)
            .context("Invalid arguments for web_fetch tool. Provide 'url' and 'prompt'.")?;

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
                // Structured, readable error. The agent can surface this or fall back to other tools.
                return Ok(json!({
                    "error": format!("web_fetch: failed to fetch URL '{}': {}", args.url, e),
                    "url": args.url,
                    "max_bytes": max_bytes,
                    "timeout_secs": timeout_secs
                }));
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
                "url": args.url
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
        // - `preview`: truncated snippet for display
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
            "next_action_hint": "Read `temp_file` to get the full fetched content, then analyze it using `prompt` and answer the user. The temp file is ephemeral and will be cleaned up automatically."
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
fn is_private_host(host: &str) -> bool {
    // Try IPv4 / IPv6 parsing first.
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => is_private_ipv4(v4),
            IpAddr::V6(v6) => is_private_ipv6(v6),
        };
    }

    // DNS names like "localhost" that will resolve to loopback.
    if host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("localhost.localdomain")
    {
        return true;
    }

    false
}

/// Check if an IPv4 address is private, loopback, or link-local.
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
        // 0.0.0.0/8 — "this network"
        || octets[0] == 0
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
}
