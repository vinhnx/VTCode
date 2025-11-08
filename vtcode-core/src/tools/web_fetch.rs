//! WebFetch tool for fetching and analyzing web content using AI

use super::traits::Tool;
use crate::config::constants::tools;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

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
pub struct WebFetchTool;

impl WebFetchTool {
    pub fn new() -> Self {
        Self
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
        if !url.starts_with("https://") {
            return Err(anyhow!("Only HTTPS URLs are allowed for security"));
        }

        // Check for localhost and private networks
        let url_lower = url.to_lowercase();
        if url_lower.contains("localhost")
            || url_lower.contains("127.0.0.1")
            || url_lower.contains("0.0.0.0")
            || url_lower.contains("::1")
            || url_lower.contains(".local")
            || url_lower.contains(".internal")
        {
            return Err(anyhow!("Access to local/private networks is blocked"));
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
}
