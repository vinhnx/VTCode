/// High-level LM Studio client for server interaction and model management.
/// Adapted from OpenAI Codex's codex-rs/lmstudio/src/client.rs
use std::io;
use std::path::Path;
use std::time::Duration;

use serde_json::Value as JsonValue;

const LMSTUDIO_CONNECTION_ERROR: &str =
    "LM Studio is not responding. Install from https://lmstudio.ai/download and run 'lms server start'.";

/// Client for interacting with a local LM Studio instance.
#[derive(Clone)]
pub struct LMStudioClient {
    client: reqwest::Client,
    base_url: String,
}

impl LMStudioClient {
    /// Create a client from a base URL and verify the server is reachable.
    pub async fn try_from_base_url(base_url: &str) -> io::Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let instance = Self {
            client,
            base_url: base_url.to_string(),
        };

        instance.check_server().await?;
        Ok(instance)
    }

    /// Verify that the server is reachable.
    async fn check_server(&self) -> io::Result<()> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let response = self.client.get(&url).send().await;

        if let Ok(resp) = response {
            if resp.status().is_success() {
                Ok(())
            } else {
                Err(io::Error::other(format!(
                    "Server returned error: {} {LMSTUDIO_CONNECTION_ERROR}",
                    resp.status()
                )))
            }
        } else {
            Err(io::Error::other(LMSTUDIO_CONNECTION_ERROR))
        }
    }

    /// Fetch the list of model IDs available on the server.
    pub async fn fetch_models(&self) -> io::Result<Vec<String>> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| io::Error::other(format!("Request failed: {e}")))?;

        if response.status().is_success() {
            let json: JsonValue = response.json().await.map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("JSON parse error: {e}"))
            })?;

            let models = json["data"]
                .as_array()
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "No 'data' array in response")
                })?
                .iter()
                .filter_map(|model| model["id"].as_str())
                .map(std::string::ToString::to_string)
                .collect();

            Ok(models)
        } else {
            Err(io::Error::other(format!(
                "Failed to fetch models: {}",
                response.status()
            )))
        }
    }

    /// Load a model by sending a minimal request (pre-loads into memory).
    pub async fn load_model(&self, model: &str) -> io::Result<()> {
        let url = format!("{}/responses", self.base_url.trim_end_matches('/'));
        let request_body = serde_json::json!({
            "model": model,
            "input": "",
            "max_output_tokens": 1
        });

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| io::Error::other(format!("Request failed: {e}")))?;

        if response.status().is_success() {
            tracing::info!("Successfully loaded model '{model}'");
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "Failed to load model: {}",
                response.status()
            )))
        }
    }

    /// Find the `lms` CLI tool, checking PATH and fallback locations.
    fn find_lms() -> io::Result<String> {
        Self::find_lms_with_home_dir(None)
    }

    /// Find `lms` CLI with an optional home directory override (for testing).
    fn find_lms_with_home_dir(home_dir: Option<&str>) -> io::Result<String> {
        // First try 'lms' in PATH
        if which::which("lms").is_ok() {
            return Ok("lms".to_string());
        }

        // Platform-specific fallback paths
        let home = match home_dir {
            Some(dir) => dir.to_string(),
            None => {
                #[cfg(unix)]
                {
                    std::env::var("HOME").unwrap_or_default()
                }
                #[cfg(windows)]
                {
                    std::env::var("USERPROFILE").unwrap_or_default()
                }
            }
        };

        #[cfg(unix)]
        let fallback_path = format!("{home}/.lmstudio/bin/lms");
        #[cfg(windows)]
        let fallback_path = format!("{home}/.lmstudio/bin/lms.exe");

        if Path::new(&fallback_path).exists() {
            Ok(fallback_path)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "LM Studio not found. Please install LM Studio from https://lmstudio.ai/",
            ))
        }
    }

    /// Download a model using the `lms` CLI tool.
    pub async fn download_model(&self, model: &str) -> io::Result<()> {
        let lms = Self::find_lms()?;
        eprintln!("Downloading model: {model}");

        let status = std::process::Command::new(&lms)
            .args(["get", "--yes", model])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| {
                io::Error::other(format!(
                    "Failed to execute '{lms} get --yes {model}': {e}"
                ))
            })?;

        if !status.success() {
            return Err(io::Error::other(format!(
                "Model download failed with exit code: {}",
                status.code().unwrap_or(-1)
            )));
        }

        tracing::info!("Successfully downloaded model '{model}'");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_lms() {
        let result = LMStudioClient::find_lms();
        match result {
            Ok(_) => {
                // lms was found in PATH - that's fine
            }
            Err(e) => {
                // Expected error when LM Studio not installed
                assert!(e.to_string().contains("LM Studio not found"));
            }
        }
    }

    #[test]
    fn test_find_lms_with_mock_home() {
        // Test fallback path construction without touching env vars
        #[cfg(unix)]
        {
            let result = LMStudioClient::find_lms_with_home_dir(Some("/test/home"));
            if let Err(e) = result {
                assert!(e.to_string().contains("LM Studio not found"));
            }
        }
        #[cfg(windows)]
        {
            let result = LMStudioClient::find_lms_with_home_dir(Some("C:\\test\\home"));
            if let Err(e) = result {
                assert!(e.to_string().contains("LM Studio not found"));
            }
        }
    }

    #[tokio::test]
    async fn test_fetch_models_happy_path() {
        if std::env::var("CODEX_SANDBOX_NETWORK_DISABLED").is_ok() {
            return;
        }

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/models"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_raw(
                    serde_json::json!({
                        "data": [
                            {"id": "openai/gpt-oss-20b"},
                        ]
                    })
                    .to_string(),
                    "application/json",
                ),
            )
            .mount(&server)
            .await;

        let client =
            LMStudioClient::try_from_base_url(&server.uri()).await;
        assert!(client.is_ok());

        let client = client.unwrap();
        let models = client.fetch_models().await.expect("fetch models");
        assert!(models.contains(&"openai/gpt-oss-20b".to_string()));
    }

    #[tokio::test]
    async fn test_fetch_models_no_data_array() {
        if std::env::var("CODEX_SANDBOX_NETWORK_DISABLED").is_ok() {
            return;
        }

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/models"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_raw(serde_json::json!({}).to_string(), "application/json"),
            )
            .mount(&server)
            .await;

        let client = LMStudioClient::try_from_base_url(&server.uri()).await;
        let client = client.unwrap();
        let result = client.fetch_models().await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No 'data' array in response"));
    }

    #[tokio::test]
    async fn test_check_server_happy_path() {
        if std::env::var("CODEX_SANDBOX_NETWORK_DISABLED").is_ok() {
            return;
        }

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/models"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let result = LMStudioClient::try_from_base_url(&server.uri()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_check_server_error() {
        if std::env::var("CODEX_SANDBOX_NETWORK_DISABLED").is_ok() {
            return;
        }

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/models"))
            .respond_with(wiremock::ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let result = LMStudioClient::try_from_base_url(&server.uri()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Server returned error: 404"));
    }
}
