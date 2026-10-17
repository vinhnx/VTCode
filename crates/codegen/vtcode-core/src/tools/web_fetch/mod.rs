//! WebFetch tool for fetching and analyzing web content using AI
//!
//! Supports both restricted (blocklist) and whitelist (allowlist) modes
//! with dynamic configuration loading from vtcode.toml
//!
//! Fetched content is written to ephemeral cache files under the user cache
//! directory's `web-fetch/` path.
//! and cleaned up periodically. Content is never persisted to the workspace.

use super::traits::Tool;
use crate::config::constants::tools;
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use futures::stream::StreamExt;
use hashbrown::HashSet;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use url::Url;
use vtcode_commons::VtCodePaths;

pub mod classify_helpers;
pub mod domains;
mod errors;
mod html_preview;
mod network_guards;
pub use classify_helpers::extract_http_status;
pub use domains::{BUILTIN_BLOCKED_DOMAINS, BUILTIN_BLOCKED_PATTERNS, MALICIOUS_PATTERNS};
use errors::web_fetch_error_response;
pub(super) use network_guards::is_private_host;
use network_guards::{domain_matches_allowed, extract_domain};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_CONTENT_SIZE: usize = 500_000; // 500KB max content size
const MAX_ALLOWED_BYTES: usize = 2_000_000; // 2MB hard cap
const MAX_ALLOWED_TIMEOUT_SECS: u64 = 120; // 2 minutes hard cap

/// Subdirectory under the user cache directory for ephemeral web-fetch artifacts.
const TEMP_SUBDIR: &str = "web_fetch";

/// Max age in seconds before temp files are cleaned up (1 hour).
const TEMP_MAX_AGE_SECS: u64 = 3600;

pub(crate) const WEB_FETCH_DESCRIPTION: &str = "Fetch a remote URL and return the start of the page inline. The result contains `preview` (readable text for HTML pages, else the first 8000 bytes of the body), `preview_kind` (`text` or `raw`), `content_length`, and a `temp_file` path holding the full fetched body; read `temp_file` only when the preview is not enough. The tool does not analyze the page: `prompt` is returned with the result to guide your own reading. Set format=markdown to get cleaned markdown inline from the defuddle.md extraction service instead; that mode allows 1 call per session. For docs domains, try /llms.txt first: for 'abc.com', fetch https://abc.com/llms.txt before the homepage, then traverse linked URLs for relevant Markdown sources. Prefer llms.txt over llms-full.txt, which can be multi-megabyte. A body larger than max_bytes is cut off and the result reports truncated_by_max_bytes and source_size_bytes, so you can retry with a larger max_bytes. Temp files are ephemeral and may be cleaned up after about an hour.";

/// Parameter schema for `web_fetch`, shared by the distributed builtin
/// registration and the web tool pack so the two surfaces cannot drift.
///
/// `format` must be declared here: argument validation rejects unknown
/// properties (`additionalProperties: false`), so an undeclared `format`
/// would make the markdown route in [`WebFetchTool::execute`] unreachable.
pub(crate) fn web_fetch_parameter_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "URL to fetch (HTTPS required by default)"
            },
            "prompt": {
                "type": "string",
                "description": "Question or instruction describing what to look for in the fetched content. It is returned with the result to guide your analysis; omit it to get a default summarize instruction."
            },
            "format": {
                "type": "string",
                "enum": ["summary", "markdown"],
                "description": "Output mode (default: summary). summary fetches the page directly and returns a preview plus a temp_file path. markdown returns the page as cleaned markdown inline via the defuddle.md extraction service; it allows 1 call per session, accepts only remote http(s) URLs, caps max_bytes at 262144, and ignores prompt and timeout_secs."
            },
            "max_bytes": {
                "type": "integer",
                "description": "Maximum response body size in bytes (default: 500000, max: 2000000). The default fits most pages, including llms.txt, so set this only to raise the limit after a truncated_by_max_bytes result or to cap a very large page."
            },
            "timeout_secs": {
                "type": "integer",
                "description": "Request timeout in seconds (default: 30, max: 120)"
            }
        },
        "required": ["url"],
        "additionalProperties": false
    })
}

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
            content: String::from_utf8(bytes.to_vec()).context("Response body is not valid UTF-8")?,
            truncated_by_max_bytes,
            source_size_bytes,
        });
    }

    // Truncate to `max_bytes`, then back off to the nearest valid UTF-8
    // boundary in a single O(n) pass via `valid_up_to`. The previous per-byte
    // `end -= 1` loop was O(n^2) on adversarial bodies (e.g. a multi-MB
    // document whose first max_bytes bytes are not UTF-8-aligned).
    let valid_end = match std::str::from_utf8(&bytes[..max_bytes]) {
        Ok(_) => max_bytes,
        Err(err) => err.valid_up_to(),
    };

    let content = String::from_utf8(bytes[..valid_end].to_vec()).context("Response body is not valid UTF-8")?;
    Ok(FetchedWebContent { content, truncated_by_max_bytes, source_size_bytes })
}

/// Returns the path to the ephemeral temp directory for web_fetch artifacts.
/// Creates the directory if it doesn't exist.
async fn web_fetch_temp_dir() -> Result<PathBuf> {
    VtCodePaths::resolve()
        .context("Could not resolve VT Code cache directory")?
        .ensure_cache_child_dir(TEMP_SUBDIR)
        .context("Failed to create web-fetch cache directory")
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

    let bytes = content.as_bytes().to_vec();
    let write_path = file_path.clone();
    tokio::task::spawn_blocking(move || VtCodePaths::write_private_file_atomic(&write_path, &bytes))
        .await
        .context("web-fetch cache writer task panicked")?
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

    async fn fetch_url_content(&self, url: &str, max_bytes: usize, timeout_secs: u64) -> Result<FetchedWebContent> {
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
            return Err(anyhow!("HTTP request failed with status: {}", response.status()));
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
        //
        // Reject up-front when Content-Length already exceeds the cap, and
        // stream `chunk()`s capped at max_bytes instead of buffering the entire
        // body with `response.bytes()`. This bounds peak memory even for
        // hostile servers that lie about or omit Content-Length.
        if let Some(content_length) = response.content_length()
            && content_length > max_bytes as u64
        {
            return Err(anyhow!(
                "Response Content-Length {} exceeds max_bytes {}; refusing to buffer the full body",
                content_length,
                max_bytes
            ));
        }

        let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
        let mut stream = response.bytes_stream();
        let mut max_reached = false;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read response body")?;
            if bytes.len() >= max_bytes {
                max_reached = true;
                break;
            }
            let take = chunk.len().min(max_bytes - bytes.len());
            bytes.extend_from_slice(&chunk[..take]);
        }

        let mut fetched = fetched_content_from_bytes(&bytes, max_bytes)?;
        // The stream stopped exactly at the cap, so the true body is at least
        // max_bytes. `fetched_content_from_bytes` compares against `bytes.len()`
        // and would report "not truncated" for a body that ends precisely at the
        // cap — override so callers still see the truncation metadata.
        if max_reached || bytes.len() >= max_bytes {
            fetched.truncated_by_max_bytes = true;
        }
        Ok(fetched)
    }

    fn validate_url(&self, url: &str) -> Result<()> {
        let parsed = Url::parse(url).map_err(|error| {
            anyhow!(
                "web_fetch only accepts remote HTTP(S) URLs; use read_file or unified_file to read local files: {error}"
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            bail!("web_fetch only accepts remote HTTP(S) URLs; use read_file or unified_file to read local files");
        }

        // HTTPS enforcement (can be disabled only for testing)
        if self.strict_https_only && parsed.scheme() != "https" {
            return Err(anyhow!("Only HTTPS URLs are allowed for security"));
        }

        // Parse the URL to extract the real host, which correctly separates
        // userinfo credentials (http://evil@127.0.0.1/) from the host.
        let domain = extract_domain(url).map_err(|e| anyhow!("Failed to parse URL for security validation: {e}"))?;

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
                return Err(anyhow!("URL contains potentially malicious pattern. Access blocked for safety"));
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
        let media_type = content_type_lower.split(';').next().unwrap_or(&content_type_lower).trim();

        if allowed_types.contains(&media_type) {
            Ok(())
        } else {
            Err(anyhow!(
                "Content type '{content_type}' is not supported. Only text-based content types are allowed."
            ))
        }
    }

    async fn run(&self, raw_args: Value) -> Result<Value> {
        let args: WebFetchArgs = serde_json::from_value(raw_args)
            .context("Invalid arguments for web_fetch tool. Provide 'url' (and optionally 'prompt').")?;

        let max_bytes = args.max_bytes.map(|v| v.min(MAX_ALLOWED_BYTES)).unwrap_or(MAX_CONTENT_SIZE);
        let timeout_secs = args
            .timeout_secs
            .map(|v| v.min(MAX_ALLOWED_TIMEOUT_SECS))
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        // Fetch the URL content with detailed error handling
        let fetched = match self.fetch_url_content(&args.url, max_bytes, timeout_secs).await {
            Ok(fetched) => fetched,
            Err(e) => {
                // Categorize the error so the agent loop can react. We used to
                // surface a flat string here, which led to the agent in
                // turn_578 reporting "github.com, npmjs.com, and crates.io are
                // all blocked" when the actual errors were 403/404 from the
                // upstream services.
                return Ok(web_fetch_error_response(&args.url, max_bytes, timeout_secs, &e));
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

        // Preview prefers extracted readable text for HTML pages (scripts,
        // styles, and head metadata removed); other bodies preview raw.
        // The full body always lands in `temp_file` untouched.
        let preview_limit = 8000;
        let extracted_text = if html_preview::looks_like_html(&content) {
            let text = html_preview::extract_text_preview(&content);
            (!text.is_empty()).then_some(text)
        } else {
            None
        };
        let (preview, truncated, preview_kind, preview_basis_len) = match extracted_text {
            Some(text) => {
                let basis_len = text.len();
                let truncated = basis_len > preview_limit;
                let preview = if truncated {
                    vtcode_commons::formatting::truncate_byte_budget(&text, preview_limit, "...")
                } else {
                    text
                };
                (preview, truncated, "text", basis_len)
            }
            None => {
                let truncated = content_length > preview_limit;
                let preview = if truncated {
                    vtcode_commons::formatting::truncate_byte_budget(&content, preview_limit, "...")
                } else {
                    content.clone()
                };
                (preview, truncated, "raw", content_length)
            }
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
            "preview_kind": preview_kind,
            "content_length": content_length,
            "truncated": truncated,
            "no_spool": true,
            "next_action_hint": if preview_kind == "text" {
                "Analyze the inline `preview` (readable text extracted from the page; scripts and styles removed) using `prompt` and answer the user directly. Only read `temp_file` (raw HTML) if you need content beyond the preview; it is ephemeral and may already be cleaned up."
            } else {
                "Analyze the inline `preview` (the start of the page) using `prompt` and answer the user directly. Only read `temp_file` if you need content beyond the preview; it is ephemeral and may already be cleaned up."
            }
        });

        // Add overflow indicator if preview was truncated
        if truncated {
            response["overflow"] = json!(format!("[+{} more characters]", preview_basis_len - preview_limit));
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
        headers.insert(USER_AGENT, HeaderValue::from_static("VT Code/1.0 (compatible; web-fetch tool)"));
        headers
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    async fn execute(&self, mut args: Value) -> Result<Value> {
        // Validate before dispatching to either backend. The markdown path is
        // a convenience route through Defuddle, but it must have the same
        // remote-only and SSRF policy as the normal fetch path.
        if let Some(url) = args.get("url").and_then(Value::as_str) {
            let max_bytes = args
                .get("max_bytes")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(MAX_CONTENT_SIZE);
            let timeout_secs = args.get("timeout_secs").and_then(Value::as_u64).unwrap_or(DEFAULT_TIMEOUT_SECS);
            if let Err(error) = self.validate_url(url) {
                return Ok(web_fetch_error_response(url, max_bytes, timeout_secs, &error));
            }
        }

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

#[cfg(test)]
mod tests;
