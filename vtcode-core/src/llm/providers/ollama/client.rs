/// High-level Ollama client for server interaction and model management.
/// Adapted from OpenAI Codex's codex-ollama/src/client.rs
use std::io;
use std::time::Duration;

use futures::StreamExt;
use futures::stream::BoxStream;
use semver::Version;
use serde_json::Value as JsonValue;

use super::pull::{OllamaPullEvent, OllamaPullProgressReporter};
use super::url::base_url_to_host_root;

/// Client for interacting with a local or remote Ollama instance.
pub struct OllamaClient {
    client: reqwest::Client,
    host_root: String,
}

const OLLAMA_CONNECTION_ERROR: &str = "No running Ollama server detected. Start it with: `ollama serve` (after installing)\n\
     Install instructions: https://github.com/ollama/ollama?tab=readme-ov-file";

impl OllamaClient {
    /// Create a client from a base URL and verify the server is reachable.
    pub async fn try_from_base_url(base_url: &str) -> io::Result<Self> {
        let host_root = base_url_to_host_root(base_url);
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let instance = Self { client, host_root };
        instance.probe_server().await?;
        Ok(instance)
    }

    /// Probe whether the server is reachable.
    async fn probe_server(&self) -> io::Result<()> {
        let url = format!("{}/api/tags", self.host_root.trim_end_matches('/'));
        let resp = self.client.get(url).send().await.map_err(|err| {
            tracing::warn!("Failed to connect to Ollama server: {err:?}");
            io::Error::other(OLLAMA_CONNECTION_ERROR)
        })?;

        if resp.status().is_success() {
            Ok(())
        } else {
            tracing::warn!(
                "Failed to probe server at {}: HTTP {}",
                self.host_root,
                resp.status()
            );
            Err(io::Error::other(OLLAMA_CONNECTION_ERROR))
        }
    }

    /// Fetch the list of model names available on the server.
    pub async fn fetch_models(&self) -> io::Result<Vec<String>> {
        let tags_url = format!("{}/api/tags", self.host_root.trim_end_matches('/'));
        let resp = self
            .client
            .get(tags_url)
            .send()
            .await
            .map_err(io::Error::other)?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let val = resp.json::<JsonValue>().await.map_err(io::Error::other)?;
        let names = val
            .get("models")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(names)
    }

    /// Query the server for its version string, returning `None` when unavailable.
    /// Adapted from OpenAI Codex's codex-ollama/src/client.rs
    pub async fn fetch_version(&self) -> io::Result<Option<Version>> {
        let version_url = format!("{}/api/version", self.host_root.trim_end_matches('/'));
        let resp = self
            .client
            .get(version_url)
            .send()
            .await
            .map_err(io::Error::other)?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let val = resp.json::<JsonValue>().await.map_err(io::Error::other)?;
        let Some(version_str) = val.get("version").and_then(|v| v.as_str()).map(str::trim) else {
            return Ok(None);
        };

        let normalized = version_str.trim_start_matches('v');
        match Version::parse(normalized) {
            Ok(version) => Ok(Some(version)),
            Err(err) => {
                tracing::warn!("Failed to parse Ollama version `{version_str}`: {err}");
                Ok(None)
            }
        }
    }

    /// Start a model pull and return a stream of events.
    pub async fn pull_model_stream(
        &self,
        model: &str,
    ) -> io::Result<BoxStream<'static, OllamaPullEvent>> {
        let url = format!("{}/api/pull", self.host_root.trim_end_matches('/'));
        let resp = self
            .client
            .post(url)
            .json(&serde_json::json!({"model": model, "stream": true}))
            .send()
            .await
            .map_err(io::Error::other)?;

        if !resp.status().is_success() {
            return Err(io::Error::other(format!(
                "failed to start pull: HTTP {}",
                resp.status()
            )));
        }

        let mut stream = resp.bytes_stream();
        let mut buf = String::new();

        let s = async_stream::stream! {
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        if let Ok(text) = std::str::from_utf8(&bytes) {
                            buf.push_str(text);
                            while let Some(pos) = buf.find('\n') {
                                let line = buf.drain(..=pos).collect::<String>();
                                let text = line.trim();
                                if text.is_empty() { continue; }
                                if let Ok(value) = serde_json::from_str::<JsonValue>(text) {
                                    for ev in super::parser::pull_events_from_value(&value) {
                                        yield ev;
                                    }
                                    if let Some(err_msg) = value.get("error").and_then(|e| e.as_str()) {
                                        yield OllamaPullEvent::Error(err_msg.to_string());
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => {
                        // Connection error: end the stream.
                        return;
                    }
                }
            }
        };

        Ok(Box::pin(s))
    }

    /// High-level helper to pull a model and drive a progress reporter.
    /// Adapted from OpenAI Codex's codex-ollama/src/client.rs
    pub async fn pull_with_reporter(
        &self,
        model: &str,
        reporter: &mut dyn OllamaPullProgressReporter,
    ) -> io::Result<()> {
        reporter.on_event(&OllamaPullEvent::Status(format!(
            "Pulling model {model}..."
        )))?;
        let mut stream = self.pull_model_stream(model).await?;

        while let Some(event) = stream.next().await {
            reporter.on_event(&event)?;
            match event {
                OllamaPullEvent::Success => {
                    return Ok(());
                }
                OllamaPullEvent::Error(err) => {
                    // Empirically, ollama returns a 200 OK response even when
                    // the output stream includes an error message. Verify with:
                    //
                    // `curl -i http://localhost:11434/api/pull -d '{ "model": "foobarbaz" }'`
                    //
                    // When we see an error in the stream, we return it to the
                    // caller as an I/O error.
                    return Err(io::Error::other(err));
                }
                _ => {}
            }
        }

        // Stream ended without explicit success or error.
        Err(io::Error::other("Pull stream ended unexpectedly"))
    }
}

#[cfg(test)]
mod tests {
    use semver::Version;

    #[test]
    fn test_client_creation_requires_valid_base_url() {
        // This would require a running Ollama server to test properly.
        // For now, we verify the URL parsing logic in url.rs tests.
    }

    #[test]
    fn test_version_parsing() {
        // Test that semver::Version parses Ollama version strings correctly
        let v = Version::parse("0.14.1").expect("parse version");
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 14);
        assert_eq!(v.patch, 1);
    }

    #[test]
    fn test_version_parsing_strips_v_prefix() {
        // Ollama may return versions with 'v' prefix
        let version_str = "v0.13.4";
        let normalized = version_str.trim_start_matches('v');
        let v = Version::parse(normalized).expect("parse version");
        assert_eq!(v, Version::new(0, 13, 4));
    }
}
