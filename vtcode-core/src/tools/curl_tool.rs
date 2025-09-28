//! Sandboxed curl-like tool with strict safety guarantees

use super::traits::Tool;
use crate::config::constants::tools;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use futures::StreamExt;
use rand::{Rng, distributions::Alphanumeric};
use reqwest::{Client, Method, Url};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;
use tracing::warn;

const DEFAULT_TIMEOUT_SECS: u64 = 10;
const MAX_TIMEOUT_SECS: u64 = 30;
const DEFAULT_MAX_BYTES: usize = 64 * 1024;
const TEMP_SUBDIR: &str = "vtcode-curl";
const SECURITY_NOTICE: &str = "Sandboxed HTTPS-only curl wrapper executed. Verify the target URL and delete any temporary files under /tmp when you finish reviewing the response.";

#[derive(Debug, Deserialize)]
struct CurlToolArgs {
    url: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    max_bytes: Option<usize>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    save_response: Option<bool>,
}

/// Secure HTTP fetch tool with aggressive validation
#[derive(Clone)]
pub struct CurlTool {
    client: Client,
    temp_root: PathBuf,
}

impl CurlTool {
    pub fn new() -> Self {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .user_agent("vtcode-sandboxed-curl/0.1")
            .build()
            .unwrap_or_else(|error| {
                warn!(
                    ?error,
                    "Failed to build dedicated curl client; falling back to default"
                );
                Client::new()
            });
        let temp_root = std::env::temp_dir().join(TEMP_SUBDIR);
        Self { client, temp_root }
    }
}

impl Default for CurlTool {
    fn default() -> Self {
        Self::new()
    }
}

impl CurlTool {
    fn write_temp_file(&self, data: &[u8]) -> Result<PathBuf> {
        if !self.temp_root.exists() {
            fs::create_dir_all(&self.temp_root)
                .context("Failed to create temporary directory for curl tool")?;
        }

        let mut rng = rand::thread_rng();
        let suffix: String = (&mut rng)
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();

        let path = self
            .temp_root
            .join(format!("response-{}.txt", suffix.to_lowercase()));
        fs::write(&path, data)
            .with_context(|| format!("Failed to write temporary file at {}", path.display()))?;
        Ok(path)
    }
    async fn run(&self, raw_args: Value) -> Result<Value> {
        let args: CurlToolArgs = serde_json::from_value(raw_args)
            .context("Invalid arguments for curl tool. Provide an object with at least a 'url'.")?;

        let method = self.normalize_method(args.method)?;
        if method == Method::HEAD && args.save_response.unwrap_or(false) {
            return Err(anyhow!(
                "Cannot save a response body when performing a HEAD request. Set save_response=false or use GET."
            ));
        }

        let url = Url::parse(&args.url).context("Invalid URL provided to curl tool")?;
        self.validate_url(&url)?;

        let timeout = args
            .timeout_secs
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(MAX_TIMEOUT_SECS);
        let max_bytes = args
            .max_bytes
            .unwrap_or(DEFAULT_MAX_BYTES)
            .min(DEFAULT_MAX_BYTES);

        if max_bytes == 0 {
            return Err(anyhow!("max_bytes must be greater than zero"));
        }

        let request = self
            .client
            .request(method.clone(), url.clone())
            .timeout(Duration::from_secs(timeout))
            .header(
                reqwest::header::ACCEPT,
                "text/plain, text/*, application/json, application/xml, application/yaml",
            );

        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to execute HTTPS request to {}", url))?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("Request returned non-success status: {}", status));
        }

        if let Some(length) = response.content_length()
            && length > max_bytes as u64
        {
            return Err(anyhow!(
                "Remote response is {} bytes which exceeds the policy limit of {} bytes",
                length,
                max_bytes
            ));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        self.validate_content_type(&content_type)?;

        if method == Method::HEAD {
            return Ok(json!({
                "success": true,
                "url": url.to_string(),
                "status": status.as_u16(),
                "content_type": content_type,
                "content_length": response.content_length(),
                "security_notice": SECURITY_NOTICE,
            }));
        }

        let mut total_bytes: usize = 0;
        let mut buffer: Vec<u8> = Vec::new();
        let mut truncated = false;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes =
                chunk.with_context(|| format!("Failed to read response chunk from {}", url))?;
            total_bytes = total_bytes.saturating_add(bytes.len());
            if buffer.len() < max_bytes {
                let remaining = max_bytes - buffer.len();
                if bytes.len() > remaining {
                    buffer.extend_from_slice(&bytes[..remaining]);
                    truncated = true;
                } else {
                    buffer.extend_from_slice(&bytes);
                }
            } else {
                truncated = true;
            }
            if buffer.len() >= max_bytes {
                truncated = true;
                break;
            }
        }

        let body_text = String::from_utf8_lossy(&buffer).to_string();
        let saved_path = if args.save_response.unwrap_or(false) && !buffer.is_empty() {
            Some(self.write_temp_file(&buffer)?)
        } else {
            None
        };

        let saved_path_str = saved_path.as_ref().map(|path| path.display().to_string());
        let cleanup_hint = saved_path
            .as_ref()
            .map(|path| format!("rm {}", path.display()));

        Ok(json!({
            "success": true,
            "url": url.to_string(),
            "status": status.as_u16(),
            "content_type": content_type,
            "bytes_read": total_bytes,
            "body": body_text,
            "truncated": truncated,
            "saved_path": saved_path_str,
            "cleanup_hint": cleanup_hint,
            "security_notice": SECURITY_NOTICE,
        }))
    }

    fn normalize_method(&self, method: Option<String>) -> Result<Method> {
        let requested = method.unwrap_or_else(|| "GET".to_string());
        let normalized = requested.trim().to_uppercase();
        match normalized.as_str() {
            "GET" => Ok(Method::GET),
            "HEAD" => Ok(Method::HEAD),
            other => Err(anyhow!(
                "HTTP method '{}' is not permitted. Only GET or HEAD are allowed.",
                other
            )),
        }
    }

    fn validate_url(&self, url: &Url) -> Result<()> {
        if url.scheme() != "https" {
            return Err(anyhow!("Only HTTPS URLs are allowed"));
        }

        if !url.username().is_empty() || url.password().is_some() {
            return Err(anyhow!("Credentials in URLs are not supported"));
        }

        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("URL must include a host"))?
            .to_lowercase();

        if host.parse::<IpAddr>().is_ok() {
            return Err(anyhow!("IP address targets are blocked for security"));
        }

        let forbidden_hosts = ["localhost", "127.0.0.1", "0.0.0.0", "::1"];

        if forbidden_hosts
            .iter()
            .any(|blocked| host == *blocked || host.ends_with(&format!(".{}", blocked)))
        {
            return Err(anyhow!("Access to local or loopback hosts is blocked"));
        }

        let forbidden_suffixes = [".localhost", ".local", ".internal", ".lan"];
        if forbidden_suffixes
            .iter()
            .any(|suffix| host.ends_with(suffix))
        {
            return Err(anyhow!("Private network hosts are not permitted"));
        }

        if let Some(port) = url.port()
            && port != 443
        {
            return Err(anyhow!("Custom HTTPS ports are blocked by policy"));
        }

        Ok(())
    }

    fn validate_content_type(&self, content_type: &str) -> Result<()> {
        if content_type.is_empty() {
            return Ok(());
        }
        let lowered = content_type.to_lowercase();
        let allowed = lowered.starts_with("text/")
            || lowered.contains("json")
            || lowered.contains("xml")
            || lowered.contains("yaml")
            || lowered.contains("toml")
            || lowered.contains("javascript");
        if allowed {
            Ok(())
        } else {
            Err(anyhow!(
                "Content type '{}' is not allowed. Only text or structured text responses are supported.",
                content_type
            ))
        }
    }
}

#[async_trait]
impl Tool for CurlTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        self.run(args).await
    }

    fn name(&self) -> &'static str {
        tools::CURL
    }

    fn description(&self) -> &'static str {
        "Fetches HTTPS text content with strict validation and security notices."
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn rejects_non_https_urls() {
        let tool = CurlTool::new();
        let result = tool
            .execute(json!({
                "url": "http://example.com"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_local_targets() {
        let tool = CurlTool::new();
        let result = tool
            .execute(json!({
                "url": "https://localhost/resource"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_disallowed_methods() {
        let tool = CurlTool::new();
        let result = tool
            .execute(json!({
                "url": "https://example.com/resource",
                "method": "POST"
            }))
            .await;
        assert!(result.is_err());
    }
}
