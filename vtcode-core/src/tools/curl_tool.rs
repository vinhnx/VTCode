//! Sandboxed curl-like tool with strict safety guarantees

use super::traits::Tool;
use crate::config::constants::tools;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use curl::easy::{Easy, List};
use rand::{Rng, distributions::Alphanumeric};
use serde::Deserialize;
use serde_json::{Value, json};

use std::io::Read;
use std::net::IpAddr;
use std::path::PathBuf;

const DEFAULT_TIMEOUT_SECS: u64 = 10;
const MAX_TIMEOUT_SECS: u64 = 30;
const DEFAULT_MAX_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024; // 1MB max for request body
const TEMP_SUBDIR: &str = "vtcode-curl";
const SECURITY_NOTICE: &str = "Sandboxed HTTPS-only curl wrapper executed. Verify the target URL and delete any temporary files under /tmp when you finish reviewing the response.";

#[derive(Debug, Deserialize)]
struct CurlToolArgs {
    url: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    headers: Option<Vec<String>>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    max_bytes: Option<usize>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    follow_redirects: Option<bool>,
    #[serde(default)]
    save_response: Option<bool>,
}

/// Secure HTTP fetch tool with aggressive validation
#[derive(Clone)]
pub struct CurlTool {
    temp_root: PathBuf,
}

impl CurlTool {
    pub fn new() -> Self {
        let temp_root = std::env::temp_dir().join(TEMP_SUBDIR);
        Self { temp_root }
    }
}

impl Default for CurlTool {
    fn default() -> Self {
        Self::new()
    }
}

impl CurlTool {
    async fn write_temp_file(&self, data: &[u8]) -> Result<PathBuf> {
        let suffix: String = {
            let mut rng = rand::thread_rng();
            (&mut rng)
                .sample_iter(&Alphanumeric)
                .take(10)
                .map(char::from)
                .collect()
        };

        if !self.temp_root.exists() {
            tokio::fs::create_dir_all(&self.temp_root)
                .await
                .context("Failed to create temporary directory for curl tool")?;
        }

        let path = self
            .temp_root
            .join(format!("response-{}.txt", suffix.to_lowercase()));
        tokio::fs::write(&path, data)
            .await
            .with_context(|| format!("Failed to write temporary file at {}", path.display()))?;
        Ok(path)
    }

    async fn run(&self, raw_args: Value) -> Result<Value> {
        let args: CurlToolArgs = serde_json::from_value(raw_args)
            .context("Invalid arguments for curl tool. Provide an object with at least a 'url'.")?;

        let method = self.normalize_method(args.method)?;
        if method == "HEAD" && args.save_response.unwrap_or(false) {
            return Err(anyhow!(
                "Cannot save a response body when performing a HEAD request. Set save_response=false or use GET."
            ));
        }

        self.validate_url(&args.url)?;

        // Validate request body: only allow for methods that can have request bodies
        if args.body.is_some() && !matches!(method, "POST" | "PUT" | "PATCH") {
            return Err(anyhow!(
                "Request body is not allowed for method '{}'. Request bodies are only allowed for POST, PUT, PATCH.",
                method
            ));
        }

        // Validate request body size if present, especially for methods that use request bodies
        if let Some(body) = &args.body {
            if body.len() > MAX_REQUEST_BODY_BYTES {
                return Err(anyhow!(
                    "Request body size {} bytes exceeds maximum allowed size of {} bytes",
                    body.len(),
                    MAX_REQUEST_BODY_BYTES
                ));
            }
        }

        let timeout = args
            .timeout_secs
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(MAX_TIMEOUT_SECS);
        let max_bytes = args
            .max_bytes
            .unwrap_or(DEFAULT_MAX_BYTES)
            .min(DEFAULT_MAX_BYTES);
        let follow_redirects = args.follow_redirects.unwrap_or(false);

        if max_bytes == 0 {
            return Err(anyhow!("max_bytes must be greater than zero"));
        }

        let url = args.url.clone();
        let is_head = method == "HEAD";
        let headers = args.headers.clone();
        let body = args.body.clone();

        let result = tokio::task::spawn_blocking(move || {
            let mut easy = Easy::new();
            easy.url(&url)?;
            easy.timeout(std::time::Duration::from_secs(timeout))?;
            easy.useragent("vtcode-sandboxed-curl/0.1")?;
            easy.follow_location(follow_redirects)?;
            easy.ssl_verify_peer(true)?;
            easy.ssl_verify_host(true)?;

            // Additional security: Disable signals and enable verbose SSL info for debugging if needed
            easy.signal(false)?; // Disable signal handling

            // Set custom headers if provided
            if let Some(headers) = headers {
                let mut list = List::new();
                for header in headers {
                    // Skip header validation as curl will handle it
                    list.append(&header)
                        .with_context(|| format!("Invalid header: {}", header))?;
                }
                easy.http_headers(list)?;
            }

            // Handle different HTTP methods
            match method {
                "GET" => {
                    // GET is default, no need to explicitly set
                }
                "HEAD" => {
                    easy.nobody(true)?;
                }
                "POST" => {
                    easy.post(true)?;
                    if let Some(ref body) = body {
                        easy.post_fields_copy(body.as_bytes())?;
                    }
                }
                "PUT" => {
                    easy.put(true)?;
                    if let Some(ref body) = body {
                        easy.upload(true)?; // Enable upload mode
                        easy.in_filesize(body.len() as u64)?;
                    }
                }
                "DELETE" => {
                    easy.custom_request("DELETE")?;
                }
                "PATCH" => {
                    easy.custom_request("PATCH")?;
                    if let Some(ref body) = body {
                        easy.post_fields_copy(body.as_bytes())?;
                    }
                }
                _ => {
                    return Err(anyhow!("Unsupported HTTP method: {}", method));
                }
            }

            let mut buffer = Vec::new();
            let mut status_code = 0u32;
            let mut content_type = String::new();
            let mut content_length: Option<u64> = None;
            let mut response_headers = Vec::new(); // Store all response headers
            let mut hit_size_limit = false; // Track if we stopped reading due to size limit

            {
                let mut transfer = easy.transfer();

                transfer.header_function(|header| {
                    if let Ok(header_str) = std::str::from_utf8(header) {
                        // Store all headers for the response
                        if !header_str.starts_with("HTTP/") {
                            response_headers.push(header_str.trim().to_string());
                        }

                        if header_str.starts_with("HTTP/") {
                            if let Some(code_str) = header_str.split_whitespace().nth(1) {
                                status_code = code_str.parse().unwrap_or(0);
                            }
                        } else if let Some(value) = header_str.strip_prefix("content-type:") {
                            content_type = value.trim().to_string();
                        } else if let Some(value) = header_str.strip_prefix("Content-Type:") {
                            content_type = value.trim().to_string();
                        } else if let Some(value) = header_str.strip_prefix("content-length:") {
                            content_length = value.trim().parse().ok();
                        } else if let Some(value) = header_str.strip_prefix("Content-Length:") {
                            content_length = value.trim().parse().ok();
                        }
                    }
                    true
                })?;

                transfer.write_function(|data| {
                    if buffer.len() < max_bytes {
                        let remaining = max_bytes - buffer.len();
                        if data.len() > remaining {
                            buffer.extend_from_slice(&data[..remaining]);
                            hit_size_limit = true;
                        } else {
                            buffer.extend_from_slice(data);
                        }
                    } else {
                        hit_size_limit = true;
                    }
                    Ok(data.len())
                })?;

                // If PUT request with body, we need to provide the data
                if method == "PUT" {
                    if let Some(ref body) = body {
                        let mut body_cursor = std::io::Cursor::new(body.clone().into_bytes());
                        transfer.read_function(move |buf| {
                            let read = body_cursor.read(buf).unwrap_or(0);
                            Ok(read)
                        })?;
                    }
                }

                transfer.perform()?;
                drop(transfer);
            }

            // Get timing information after the request is complete
            let total_time = easy.total_time()?.as_secs_f64();

            Ok::<_, anyhow::Error>((
                status_code,
                content_type,
                content_length,
                buffer,
                response_headers,
                total_time,
                hit_size_limit,
            ))
        })
        .await
        .context("Failed to execute curl request")??;

        let (
            status,
            content_type,
            content_length,
            buffer,
            response_headers,
            total_time,
            hit_size_limit,
        ) = result;

        if status < 200 || status >= 300 {
            return Err(anyhow!("Request returned non-success status: {}", status));
        }

        if let Some(length) = content_length {
            if length > max_bytes as u64 {
                return Err(anyhow!(
                    "Remote response is {} bytes which exceeds the policy limit of {} bytes",
                    length,
                    max_bytes
                ));
            }
        }

        self.validate_content_type(&content_type)?;

        if is_head {
            return Ok(json!({
                "success": true,
                "url": args.url,
                "status": status,
                "content_type": content_type,
                "content_length": content_length,
                "headers": response_headers,
                "total_time": total_time,
                "security_notice": SECURITY_NOTICE,
            }));
        }

        // Determine if response was truncated - either hit size limit or Content-Length indicates more data
        let truncated =
            hit_size_limit || content_length.map_or(false, |len| len > buffer.len() as u64);
        let body_text = String::from_utf8_lossy(&buffer).to_string();
        let saved_path = if args.save_response.unwrap_or(false) && !buffer.is_empty() {
            Some(self.write_temp_file(&buffer).await?)
        } else {
            None
        };

        let saved_path_str = saved_path.as_ref().map(|path| path.display().to_string());
        let cleanup_hint = saved_path
            .as_ref()
            .map(|path| format!("rm {}", path.display()));

        Ok(json!({
            "success": true,
            "url": args.url,
            "method": method,
            "status": status,
            "content_type": content_type,
            "bytes_read": buffer.len(),
            "body": body_text,
            "truncated": truncated,
            "followed_redirects": follow_redirects,
            "headers": response_headers,
            "total_time": total_time,
            "saved_path": saved_path_str,
            "cleanup_hint": cleanup_hint,
            "security_notice": SECURITY_NOTICE,
        }))
    }

    fn normalize_method(&self, method: Option<String>) -> Result<&'static str> {
        let requested = method.unwrap_or_else(|| "GET".to_string());
        let normalized = requested.trim().to_uppercase();
        match normalized.as_str() {
            "GET" => Ok("GET"),
            "HEAD" => Ok("HEAD"),
            "POST" => Ok("POST"),
            "PUT" => Ok("PUT"),
            "DELETE" => Ok("DELETE"),
            "PATCH" => Ok("PATCH"),
            other => Err(anyhow!(
                "HTTP method '{}' is not permitted. Only GET, HEAD, POST, PUT, DELETE, PATCH are allowed.",
                other
            )),
        }
    }

    fn validate_url(&self, url: &str) -> Result<()> {
        if !url.starts_with("https://") {
            return Err(anyhow!("Only HTTPS URLs are allowed"));
        }

        let url_lower = url.to_lowercase();

        if url_lower.contains('@') {
            return Err(anyhow!("Credentials in URLs are not supported"));
        }

        let host = url
            .strip_prefix("https://")
            .and_then(|s| s.split('/').next())
            .and_then(|s| s.split(':').next())
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

        // Check for custom ports - extract host:port from URL
        if let Some(after_scheme) = url.strip_prefix("https://") {
            if let Some(host_port) = after_scheme.split('/').next() {
                if let Some(port_str) = host_port.split(':').nth(1) {
                    // Found a port specification
                    if let Ok(port) = port_str.parse::<u16>() {
                        if port != 443 {
                            return Err(anyhow!("Custom HTTPS ports are blocked by policy"));
                        }
                    }
                }
            }
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
        "Fetches HTTPS text content with strict validation and security notices. Supports GET, HEAD, POST, PUT, DELETE, PATCH methods with custom headers, request bodies, timing information, and optional redirect following."
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
                "method": "CONNECT"
            }))
            .await;
        assert!(result.is_err());
    }
}
