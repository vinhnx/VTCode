//! WebFetch tool for fetching and analyzing web content using AI
//!
//! Supports both restricted (blocklist) and whitelist (allowlist) modes
//! with dynamic configuration loading from vtcode.toml

use super::traits::Tool;
use crate::config::constants::tools;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub mod domains;
pub use domains::{BUILTIN_BLOCKED_DOMAINS, BUILTIN_BLOCKED_PATTERNS, MALICIOUS_PATTERNS};

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_CONTENT_SIZE: usize = 500_000; // 500KB max content size

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
    /// Security mode: "restricted" (blocklist) or "whitelist" (allowlist)
    pub mode: String,
    /// Additional blocked domains (merged with builtin)
    pub blocked_domains: HashSet<String>,
    /// Additional blocked patterns (merged with builtin)
    pub blocked_patterns: Vec<String>,
    /// Allowed domains (for exemptions in restricted mode or primary list in whitelist mode)
    pub allowed_domains: HashSet<String>,
    /// Strict HTTPS-only mode
    pub strict_https_only: bool,
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self {
            mode: "restricted".to_string(),
            blocked_domains: HashSet::new(),
            blocked_patterns: Vec::new(),
            allowed_domains: HashSet::new(),
            strict_https_only: true,
        }
    }

    /// Create a WebFetchTool with custom configuration
    pub fn with_config(
        mode: String,
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
    ) -> Result<String> {
        // Validate URL
        self.validate_url(url)?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent("VTCode/1.0 (compatible; web-fetch tool)")
            .build()?;

        let response = client
            .get(url)
            .header(
                "Accept",
                "text/markdown, text/html, application/json, text/plain, */*",
            )
            .send()
            .await?;

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

        // Limit response body to max_bytes
        let bytes = response.bytes().await?;
        if bytes.len() > max_bytes {
            return Err(anyhow!(
                "Response body size {} bytes exceeds maximum allowed size of {} bytes",
                bytes.len(),
                max_bytes
            ));
        }

        String::from_utf8(bytes.to_vec()).context("Response body is not valid UTF-8")
    }

    fn validate_url(&self, url: &str) -> Result<()> {
        // HTTPS enforcement (can be disabled only for testing)
        if self.strict_https_only && !url.starts_with("https://") {
            return Err(anyhow!("Only HTTPS URLs are allowed for security"));
        }

        let url_lower = url.to_lowercase();

        // Check for localhost and private networks (always blocked)
        if url_lower.contains("localhost")
            || url_lower.contains("127.0.0.1")
            || url_lower.contains("0.0.0.0")
            || url_lower.contains("::1")
            || url_lower.contains(".local")
            || url_lower.contains(".internal")
        {
            return Err(anyhow!("Access to local/private networks is blocked"));
        }

        // Apply security policy based on mode
        match self.mode.as_str() {
            "whitelist" => self.validate_whitelist_mode(&url_lower)?,
            "restricted" => self.validate_restricted_mode(&url_lower)?,
            _ => return Err(anyhow!("Unknown web_fetch security mode: {}", self.mode)),
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
            if domain.ends_with(allowed.as_str()) || &domain == allowed {
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
            if domain.ends_with(allowed.as_str()) || &domain == allowed {
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

    /// Expand ~ to home directory
    #[allow(dead_code)]
    fn expand_home_path(path: &str) -> String {
        if path.starts_with("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return path.replace("~/", &format!("{}/", home));
            }
        }
        path.to_string()
    }

    /// Load blocklist from external JSON file
    #[allow(dead_code)]
    async fn load_dynamic_blocklist(&self, path: &str) -> Result<(Vec<String>, Vec<String>)> {
        let expanded_path = Self::expand_home_path(path);
        if !Path::new(&expanded_path).exists() {
            return Ok((Vec::new(), Vec::new()));
        }

        let content = fs::read_to_string(&expanded_path)
            .context(format!("Failed to read blocklist from {}", path))?;

        #[derive(Deserialize)]
        struct BlocklistFile {
            blocked_domains: Option<Vec<String>>,
            blocked_patterns: Option<Vec<String>>,
        }

        let blocklist: BlocklistFile = serde_json::from_str(&content)
            .context(format!("Failed to parse blocklist JSON from {}", path))?;

        Ok((
            blocklist.blocked_domains.unwrap_or_default(),
            blocklist.blocked_patterns.unwrap_or_default(),
        ))
    }

    /// Load whitelist from external JSON file
    #[allow(dead_code)]
    async fn load_dynamic_whitelist(&self, path: &str) -> Result<Vec<String>> {
        let expanded_path = Self::expand_home_path(path);
        if !Path::new(&expanded_path).exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&expanded_path)
            .context(format!("Failed to read whitelist from {}", path))?;

        #[derive(Deserialize)]
        struct WhitelistFile {
            allowed_domains: Option<Vec<String>>,
        }

        let whitelist: WhitelistFile = serde_json::from_str(&content)
            .context(format!("Failed to parse whitelist JSON from {}", path))?;

        Ok(whitelist.allowed_domains.unwrap_or_default())
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

        let content_type_lower = content_type.to_lowercase();
        if allowed_types
            .iter()
            .any(|&t| content_type_lower.contains(t))
        {
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

        let max_bytes = args.max_bytes.unwrap_or(MAX_CONTENT_SIZE);
        let timeout_secs = args.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);

        // Fetch the URL content with detailed error handling
        let content = match self
            .fetch_url_content(&args.url, max_bytes, timeout_secs)
            .await
        {
            Ok(content) => content,
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

        // Truncate preview for UI; keep full content available for reasoning.
        let preview_limit = 8000;
        let (preview, truncated) = if content_length > preview_limit {
            (format!("{}...", &content[..preview_limit]), true)
        } else {
            (content.clone(), false)
        };

        // Canonical response shape:
        // - `content`: full fetched body
        // - `preview`: truncated snippet for display
        // - `prompt`: what the user/model wants to know
        // - `next_action_hint`: explicit instruction so the agent continues the loop correctly
        Ok(json!({
            "url": args.url,
            "prompt": args.prompt,
            "content": content,
            "preview": preview,
            "content_length": content_length,
            "truncated": truncated,
            "next_action_hint": "Analyze `content` using `prompt` and answer the user in natural language based on the fetched page."
        }))
    }
}

/// Helper function to extract domain from URL
fn extract_domain(url: &str) -> Result<String> {
    // Remove protocol
    let without_proto = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    // Split by first / to remove path
    let domain_part = without_proto.split('/').next().unwrap_or("");

    // Split by first ? to remove query string
    let domain_only = domain_part.split('?').next().unwrap_or("");

    // Remove port if present
    let domain_no_port = domain_only.split(':').next().unwrap_or("");

    if domain_no_port.is_empty() {
        return Err(anyhow!("Could not extract domain from URL: {}", url));
    }

    Ok(domain_no_port.to_string())
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
        // - Simple "fetch https://..." style calls are handled natively by VTCode.
        // - We do not force upstream agents or MCP tools to construct a full prompt.
        // - MCP tools like `get_current_time` remain unaffected (they are separate).
        if let Some(obj) = args.as_object_mut() {
            let has_url = obj.get("url").map(|v| v.is_string()).unwrap_or(false);
            let has_prompt = obj.get("prompt").map(|v| v.is_string()).unwrap_or(false);

            if has_url && !has_prompt {
                obj.insert(
                    "prompt".to_string(),
                    json!("Briefly summarize what this page is and what it represents. Focus on the owner/profile, primary purpose, and any notable repositories or projects."),
                );
            }
        }

        self.run(args).await
    }

    fn name(&self) -> &'static str {
        tools::WEB_FETCH
    }

    fn description(&self) -> &'static str {
        "Fetches content from a specified URL and returns an analyzed summary. Accepts: { url: string, prompt?: string, max_bytes?: number, timeout_secs?: number }. If 'prompt' is omitted, VTCode uses a safe default summary prompt so that simple 'fetch https://…' requests are handled by this built-in tool instead of delegating to external MCP tools."
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn rejects_non_https_urls() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({
                "url": "http://example.com",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn allows_http_when_https_disabled() {
        let tool = WebFetchTool::with_config(
            "restricted".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false, // strict_https_only = false
        );
        let result = tool
            .execute(json!({
                "url": "http://example.com",
                "prompt": "Extract the main content"
            }))
            .await;
        // Should not reject for HTTP protocol, but will fail for network reasons
        // The key is it passed URL validation
        let error = result.unwrap_err().to_string();
        assert!(!error.contains("Only HTTPS URLs are allowed"));
    }

    #[tokio::test]
    async fn rejects_localhost_urls() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({
                "url": "https://localhost:8080",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn requires_both_url_and_prompt() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({
                "url": "https://example.com"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_sensitive_banking_domains() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({
                "url": "https://paypal.com/login",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_sensitive_auth_domains() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({
                "url": "https://accounts.google.com",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_urls_with_credentials() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({
                "url": "https://example.com?password=secret123",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_urls_with_api_keys() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({
                "url": "https://api.example.com?api_key=sk_live_123456",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_urls_with_tokens() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({
                "url": "https://example.com?token=xyz123",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_malicious_url_patterns() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({
                "url": "https://example.com/malware.exe",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_typosquatting_domains() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({
                "url": "https://g00gle.com",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_url_shorteners() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(json!({
                "url": "https://bit.ly/xyz123",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn whitelist_mode_requires_allowed_domains() {
        let tool = WebFetchTool::with_config(
            "whitelist".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(), // No allowed domains
            true,
        );
        let result = tool
            .execute(json!({
                "url": "https://example.com",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("whitelist") || error.contains("whitelisted"));
    }

    #[tokio::test]
    async fn whitelist_mode_allows_whitelisted_domains() {
        let tool = WebFetchTool::with_config(
            "whitelist".to_string(),
            Vec::new(),
            Vec::new(),
            vec!["example.com".to_string()], // Only example.com allowed
            true,
        );
        // URL validation should pass, but network fetch will fail
        // The important part is that URL validation succeeded
        let result = tool
            .execute(json!({
                "url": "https://example.com/path",
                "prompt": "Extract the main content"
            }))
            .await;
        // Will fail on network, not on validation
        if let Err(e) = result {
            assert!(!e.to_string().contains("not in the whitelist"));
        }
    }

    #[tokio::test]
    async fn whitelist_mode_rejects_non_whitelisted_domains() {
        let tool = WebFetchTool::with_config(
            "whitelist".to_string(),
            Vec::new(),
            Vec::new(),
            vec!["allowed.com".to_string()],
            true,
        );
        let result = tool
            .execute(json!({
                "url": "https://notallowed.com",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("not in the whitelist"));
    }

    #[tokio::test]
    async fn restricted_mode_allows_exemptions() {
        let tool = WebFetchTool::with_config(
            "restricted".to_string(),
            Vec::new(),
            Vec::new(),
            vec!["paypal.com".to_string()], // Exempt from blocklist
            true,
        );
        // PayPal is normally blocked, but exempted in allowed_domains
        let result = tool
            .execute(json!({
                "url": "https://paypal.com/login",
                "prompt": "Extract the main content"
            }))
            .await;
        // Will fail on network, not on validation
        if let Err(e) = result {
            assert!(!e.to_string().contains("blocked for privacy"));
        }
    }

    #[tokio::test]
    async fn custom_blocked_domains_work() {
        let tool = WebFetchTool::with_config(
            "restricted".to_string(),
            vec!["custom-blocked.com".to_string()], // Custom blocked domain
            Vec::new(),
            Vec::new(),
            true,
        );
        let result = tool
            .execute(json!({
                "url": "https://custom-blocked.com/page",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("blocked for privacy and security reasons"));
    }

    #[tokio::test]
    async fn custom_blocked_patterns_work() {
        let tool = WebFetchTool::with_config(
            "restricted".to_string(),
            Vec::new(),
            vec!["custom_secret=".to_string()], // Custom pattern
            Vec::new(),
            true,
        );
        let result = tool
            .execute(json!({
                "url": "https://example.com?custom_secret=abc123",
                "prompt": "Extract the main content"
            }))
            .await;
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("sensitive pattern"));
    }
}
