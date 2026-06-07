use super::common::resolve_model;
use super::ollama::base_url_to_host_root;
use super::openai::OpenAIProvider;
use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, Message};
use crate::llm::providers::common::override_base_url;
use crate::utils::http_client;
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, watch};
use tokio::time::sleep;
use url::Url;

const DEFAULT_STARTUP_TIMEOUT_SECONDS: u64 = 60;
const SERVER_POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Deserialize, Serialize)]
struct LlamaCppModelsResponse {
    data: Vec<LlamaCppModel>,
}

#[derive(Debug, Deserialize, Serialize)]
struct LlamaCppModel {
    id: String,
}

const LLAMACPP_CONNECTION_ERROR: &str = "llama.cpp is not responding. Install from https://llama.app and either start `llama-server -m /path/to/model.gguf --port 8080` yourself or set LLAMACPP_MODEL_PATH so VT Code can manage startup.";

#[derive(Debug, Clone, PartialEq, Eq)]
enum ServerPhase {
    NotStarted,
    Starting,
    Ready,
    Failed,
}

#[derive(Debug, Clone)]
struct ServerStatus {
    phase: ServerPhase,
    model_id: Option<String>,
    model_path: Option<String>,
    error: Option<String>,
}

impl Default for ServerStatus {
    fn default() -> Self {
        Self {
            phase: ServerPhase::NotStarted,
            model_id: None,
            model_path: None,
            error: None,
        }
    }
}

impl ServerStatus {
    fn starting(model_path: Option<String>) -> Self {
        Self {
            phase: ServerPhase::Starting,
            model_id: None,
            model_path,
            error: None,
        }
    }

    fn ready(model_id: String, model_path: Option<String>) -> Self {
        Self {
            phase: ServerPhase::Ready,
            model_id: Some(model_id),
            model_path,
            error: None,
        }
    }

    fn failed(error: impl Into<String>, model_path: Option<String>) -> Self {
        Self {
            phase: ServerPhase::Failed,
            model_id: None,
            model_path,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug)]
struct ManagedLlamaCppServer {
    state: AsyncMutex<ManagedLlamaCppState>,
    status_tx: watch::Sender<ServerStatus>,
}

#[derive(Debug, Default)]
struct ManagedLlamaCppState {
    child: Option<Child>,
    status: ServerStatus,
}

impl ManagedLlamaCppServer {
    fn new() -> Self {
        let status = ServerStatus::default();
        let (status_tx, _) = watch::channel(status.clone());
        Self {
            state: AsyncMutex::new(ManagedLlamaCppState {
                child: None,
                status,
            }),
            status_tx,
        }
    }
}

#[derive(Debug)]
enum ServerProbe {
    Ready(String),
    Loading,
    Unavailable(String),
}

static MANAGED_LLAMACPP_SERVERS: LazyLock<Mutex<HashMap<String, Arc<ManagedLlamaCppServer>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub async fn fetch_llamacpp_models(base_url: Option<String>) -> Result<Vec<String>, anyhow::Error> {
    let resolved_base_url = override_base_url(
        urls::LLAMACPP_API_BASE,
        base_url,
        Some(env_vars::LLAMACPP_BASE_URL),
    );
    let models_url = format!("{}/models", resolved_base_url.trim_end_matches('/'));
    let client = http_client::create_client_with_timeout(Duration::from_secs(5));
    let response = client
        .get(&models_url)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| {
            tracing::warn!("Failed to connect to llama.cpp server: {e:?}");
            anyhow::anyhow!(LLAMACPP_CONNECTION_ERROR)
        })?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch llama.cpp models: HTTP {}. {}",
            response.status(),
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                "Ensure llama-server is running and exposing the OpenAI-compatible /v1 API."
            } else {
                ""
            }
        ));
    }

    let models_response: LlamaCppModelsResponse = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse llama.cpp models response: {}", e))?;

    Ok(models_response
        .data
        .into_iter()
        .map(|model| model.id)
        .collect())
}

pub struct LlamaCppProvider {
    inner: OpenAIProvider,
    api_key: Option<String>,
    configured_model: Option<String>,
    base_url: String,
    prompt_cache: Option<PromptCachingConfig>,
    timeouts: Option<TimeoutsConfig>,
    anthropic: Option<AnthropicConfig>,
    model_behavior: Option<ModelConfig>,
}

impl LlamaCppProvider {
    fn resolve_base_url(base_url: Option<String>) -> String {
        override_base_url(
            urls::LLAMACPP_API_BASE,
            base_url,
            Some(env_vars::LLAMACPP_BASE_URL),
        )
    }

    fn build_inner(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> OpenAIProvider {
        let resolved_model = resolve_model(model, models::llamacpp::DEFAULT_MODEL);
        let resolved_base = Self::resolve_base_url(base_url);
        OpenAIProvider::from_config(
            api_key,
            None,
            Some(resolved_model),
            Some(resolved_base),
            prompt_cache,
            timeouts,
            anthropic,
            None,
            model_behavior,
        )
    }

    fn managed_server_for(base_url: &str) -> Arc<ManagedLlamaCppServer> {
        let host_root = base_url_to_host_root(base_url);
        let mut guard = MANAGED_LLAMACPP_SERVERS
            .lock()
            .expect("llama.cpp managed server map poisoned");
        guard
            .entry(host_root)
            .or_insert_with(|| Arc::new(ManagedLlamaCppServer::new()))
            .clone()
    }

    fn provider_error(message: impl Into<String>) -> LLMError {
        LLMError::Provider {
            message: error_display::format_llm_error("llama.cpp", &message.into()),
            metadata: None,
        }
    }

    fn configured_startup_model_path(configured_model: Option<&str>) -> Option<String> {
        std::env::var(env_vars::LLAMACPP_MODEL_PATH)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                configured_model.and_then(|value| {
                    let trimmed = value.trim();
                    if trimmed.is_empty() || !Self::looks_like_local_model_path(trimmed) {
                        return None;
                    }
                    Some(trimmed.to_string())
                })
            })
    }

    fn startup_timeout() -> Duration {
        std::env::var(env_vars::LLAMACPP_STARTUP_TIMEOUT_SECONDS)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|seconds| *seconds > 0)
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(DEFAULT_STARTUP_TIMEOUT_SECONDS))
    }

    fn looks_like_local_model_path(value: &str) -> bool {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return false;
        }

        trimmed.ends_with(".gguf")
            || trimmed.contains(std::path::MAIN_SEPARATOR)
            || trimmed.contains('/')
            || trimmed.starts_with('.')
            || Path::new(trimmed).exists()
    }

    fn is_local_base_url(base_url: &str) -> bool {
        let host_root = base_url_to_host_root(base_url);
        Url::parse(&host_root)
            .ok()
            .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
            .is_some_and(|host| host == "localhost" || host == "127.0.0.1" || host == "::1")
    }

    fn host_port(base_url: &str) -> Result<u16> {
        let host_root = base_url_to_host_root(base_url);
        let parsed = Url::parse(&host_root)
            .with_context(|| format!("Failed to parse llama.cpp base URL: {host_root}"))?;
        Ok(parsed.port().unwrap_or(8080))
    }

    fn resolve_binary_path() -> Result<String> {
        if let Ok(path) = std::env::var(env_vars::LLAMACPP_BINARY_PATH)
            && !path.trim().is_empty()
        {
            return Ok(path);
        }

        which::which("llama-server")
            .map(|path| path.to_string_lossy().into_owned())
            .context("Could not find `llama-server` on PATH. Install llama.cpp from https://llama.app or set LLAMACPP_BINARY_PATH.")
    }

    fn build_command_args(base_url: &str, model_path: &str) -> Result<Vec<String>> {
        let path = Path::new(model_path);
        if !path.exists() {
            anyhow::bail!("Configured model path does not exist: {model_path}");
        }

        let mut args = vec![
            "-m".to_string(),
            model_path.to_string(),
            "--port".to_string(),
            Self::host_port(base_url)?.to_string(),
        ];

        if let Ok(extra_args) = std::env::var(env_vars::LLAMACPP_EXTRA_ARGS)
            && !extra_args.trim().is_empty()
        {
            args.extend(shell_words::split(&extra_args).with_context(|| {
                format!(
                    "Failed to parse {}: {extra_args}",
                    env_vars::LLAMACPP_EXTRA_ARGS
                )
            })?);
        }

        Ok(args)
    }

    async fn spawn_managed_server(base_url: &str, model_path: &str) -> Result<Child> {
        let binary = Self::resolve_binary_path()?;
        let args = Self::build_command_args(base_url, model_path)?;
        let mut command = Command::new(&binary);
        command
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        command.spawn().with_context(|| {
            format!(
                "Failed to start llama.cpp server with `{binary} {}`",
                args.join(" ")
            )
        })
    }

    async fn probe_server(base_url: &str) -> ServerProbe {
        let host_root = base_url_to_host_root(base_url);
        let health_url = format!("{}/health", host_root.trim_end_matches('/'));
        let client = http_client::create_client_with_timeout(Duration::from_secs(5));

        let response = match client.get(&health_url).send().await {
            Ok(response) => response,
            Err(error) => {
                tracing::debug!("llama.cpp health probe failed for {health_url}: {error}");
                return ServerProbe::Unavailable(LLAMACPP_CONNECTION_ERROR.to_string());
            }
        };

        if response.status().is_success() {
            return match fetch_llamacpp_models(Some(base_url.to_string())).await {
                Ok(models) if !models.is_empty() => ServerProbe::Ready(models[0].clone()),
                Ok(_) => ServerProbe::Unavailable(
                    "llama.cpp is running but did not report any loaded models from /v1/models."
                        .to_string(),
                ),
                Err(error) => ServerProbe::Unavailable(error.to_string()),
            };
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::SERVICE_UNAVAILABLE
            && body.to_ascii_lowercase().contains("loading")
        {
            return ServerProbe::Loading;
        }

        if status == reqwest::StatusCode::NOT_FOUND {
            return match fetch_llamacpp_models(Some(base_url.to_string())).await {
                Ok(models) if !models.is_empty() => ServerProbe::Ready(models[0].clone()),
                Ok(_) => ServerProbe::Unavailable(
                    "llama.cpp is running but did not report any loaded models from /v1/models."
                        .to_string(),
                ),
                Err(error) => ServerProbe::Unavailable(error.to_string()),
            };
        }

        ServerProbe::Unavailable(format!(
            "llama.cpp health check failed with HTTP {}{}",
            status,
            if body.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", body.trim())
            }
        ))
    }

    async fn wait_until_ready(base_url: &str, timeout: Duration) -> Result<String> {
        let deadline = Instant::now() + timeout;
        let mut last_error = LLAMACPP_CONNECTION_ERROR.to_string();

        while Instant::now() < deadline {
            match Self::probe_server(base_url).await {
                ServerProbe::Ready(model_id) => return Ok(model_id),
                ServerProbe::Loading => {
                    last_error = "llama.cpp is still loading the configured model".to_string();
                }
                ServerProbe::Unavailable(message) => {
                    last_error = message;
                }
            }

            sleep(SERVER_POLL_INTERVAL).await;
        }

        Err(anyhow::anyhow!(
            "Timed out waiting for llama.cpp to become ready after {}s. Last status: {}",
            timeout.as_secs(),
            last_error
        ))
    }

    async fn ensure_server_ready(&self) -> Result<String, LLMError> {
        let timeout = Self::startup_timeout();
        let initial_probe = Self::probe_server(&self.base_url).await;
        match &initial_probe {
            ServerProbe::Ready(model_id) => {
                let server = Self::managed_server_for(&self.base_url);
                let mut state = server.state.lock().await;
                state.status =
                    ServerStatus::ready(model_id.clone(), state.status.model_path.clone());
                let _ = server.status_tx.send(state.status.clone());
                return Ok(model_id.clone());
            }
            ServerProbe::Loading => {}
            ServerProbe::Unavailable(_) => {}
        }

        let startup_model_path =
            Self::configured_startup_model_path(self.configured_model.as_deref());
        let server = Self::managed_server_for(&self.base_url);
        let mut rx = server.status_tx.subscribe();

        loop {
            let mut should_spawn = false;
            {
                let mut state = server.state.lock().await;

                if let Some(child) = state.child.as_mut() {
                    match child.try_wait() {
                        Ok(Some(exit_status)) => {
                            let model_path = state.status.model_path.clone();
                            state.child = None;
                            state.status = ServerStatus::failed(
                                format!(
                                    "Managed llama.cpp server exited with status {exit_status}"
                                ),
                                model_path,
                            );
                            let _ = server.status_tx.send(state.status.clone());
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let model_path = state.status.model_path.clone();
                            state.child = None;
                            state.status = ServerStatus::failed(
                                format!("Failed to inspect managed llama.cpp server: {error}"),
                                model_path,
                            );
                            let _ = server.status_tx.send(state.status.clone());
                        }
                    }
                }

                match state.status.phase {
                    ServerPhase::Ready => {
                        if let Some(model_id) = state.status.model_id.clone() {
                            return Ok(model_id);
                        }
                        state.status = ServerStatus::default();
                        let _ = server.status_tx.send(state.status.clone());
                    }
                    ServerPhase::Starting => {}
                    ServerPhase::NotStarted | ServerPhase::Failed => {
                        match startup_model_path.clone() {
                            Some(model_path) => {
                                if !Self::is_local_base_url(&self.base_url) {
                                    return Err(Self::provider_error(format!(
                                        "{} Auto-start is only available for localhost llama.cpp endpoints.",
                                        LLAMACPP_CONNECTION_ERROR
                                    )));
                                }
                                state.status = ServerStatus::starting(Some(model_path));
                                let _ = server.status_tx.send(state.status.clone());
                                should_spawn = true;
                            }
                            None => {
                                let reason = match &initial_probe {
                                    ServerProbe::Unavailable(message) => message.clone(),
                                    ServerProbe::Loading => {
                                        "llama.cpp is still loading but no managed model path is configured"
                                            .to_string()
                                    }
                                    ServerProbe::Ready(model_id) => return Ok(model_id.clone()),
                                };
                                return Err(Self::provider_error(format!(
                                    "{reason} Set {} or configure the provider model to a local .gguf path so VT Code can launch llama-server automatically.",
                                    env_vars::LLAMACPP_MODEL_PATH
                                )));
                            }
                        }
                    }
                }
            }

            if should_spawn {
                let model_path = startup_model_path.clone().ok_or_else(|| {
                    Self::provider_error(format!(
                        "Managed llama.cpp startup requires {} or a provider model path",
                        env_vars::LLAMACPP_MODEL_PATH
                    ))
                })?;

                let spawn_result = async {
                    let child = Self::spawn_managed_server(&self.base_url, &model_path).await?;
                    let model_id = Self::wait_until_ready(&self.base_url, timeout).await?;
                    Ok::<_, anyhow::Error>((child, model_id))
                }
                .await;

                let mut state = server.state.lock().await;
                match spawn_result {
                    Ok((child, model_id)) => {
                        state.child = Some(child);
                        state.status = ServerStatus::ready(model_id.clone(), Some(model_path));
                        let _ = server.status_tx.send(state.status.clone());
                        return Ok(model_id);
                    }
                    Err(error) => {
                        state.child = None;
                        state.status = ServerStatus::failed(error.to_string(), Some(model_path));
                        let _ = server.status_tx.send(state.status.clone());
                        return Err(Self::provider_error(error.to_string()));
                    }
                }
            }

            rx.changed().await.map_err(|_| {
                Self::provider_error("llama.cpp managed server watcher unexpectedly closed")
            })?;

            let status = rx.borrow().clone();
            match status.phase {
                ServerPhase::Ready => {
                    if let Some(model_id) = status.model_id {
                        return Ok(model_id);
                    }
                }
                ServerPhase::Failed => {
                    return Err(Self::provider_error(
                        status
                            .error
                            .unwrap_or_else(|| LLAMACPP_CONNECTION_ERROR.to_string()),
                    ));
                }
                ServerPhase::Starting | ServerPhase::NotStarted => {}
            }
        }
    }

    fn should_replace_request_model(
        &self,
        request_model: &str,
        discovered_models: &[String],
    ) -> bool {
        let trimmed = request_model.trim();
        if trimmed.is_empty() || Self::looks_like_local_model_path(trimmed) {
            return true;
        }

        if discovered_models.len() == 1 {
            let configured = self
                .configured_model
                .as_deref()
                .map(str::trim)
                .unwrap_or_default();
            if trimmed == models::llamacpp::DEFAULT_MODEL || trimmed == configured {
                return true;
            }
        }

        !discovered_models.iter().any(|model| model == trimmed) && discovered_models.len() == 1
    }

    fn request_model_or_default(&self, request_model: &str) -> String {
        let trimmed = request_model.trim();
        if trimmed.is_empty() {
            resolve_model(
                self.configured_model.clone(),
                models::llamacpp::DEFAULT_MODEL,
            )
        } else {
            trimmed.to_string()
        }
    }

    fn build_request_provider(&self, model: String) -> OpenAIProvider {
        Self::build_inner(
            self.api_key.clone(),
            Some(model),
            Some(self.base_url.clone()),
            self.prompt_cache.clone(),
            self.timeouts.clone(),
            self.anthropic.clone(),
            self.model_behavior.clone(),
        )
    }

    async fn prepare_request(
        &self,
        mut request: LLMRequest,
    ) -> Result<(OpenAIProvider, LLMRequest), LLMError> {
        let discovered_model = self.ensure_server_ready().await?;
        let discovered_models = vec![discovered_model.clone()];

        if self.should_replace_request_model(&request.model, &discovered_models)
            || request.model.trim().is_empty()
        {
            request.model = discovered_model.clone();
        } else {
            request.model = self.request_model_or_default(&request.model);
        }

        Ok((self.build_request_provider(request.model.clone()), request))
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let resolved_base_url = Self::resolve_base_url(base_url.clone());
        Self {
            inner: Self::build_inner(
                api_key.clone(),
                model.clone(),
                base_url,
                prompt_cache.clone(),
                timeouts.clone(),
                anthropic.clone(),
                model_behavior.clone(),
            ),
            api_key,
            configured_model: model,
            base_url: resolved_base_url,
            prompt_cache,
            timeouts,
            anthropic,
            model_behavior,
        }
    }
}

#[async_trait]
impl LLMProvider for LlamaCppProvider {
    fn name(&self) -> &str {
        "llamacpp"
    }

    fn supports_streaming(&self) -> bool {
        self.inner.supports_streaming()
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        self.inner.supports_reasoning(model)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        self.inner.supports_reasoning_effort(model)
    }

    fn supports_tools(&self, model: &str) -> bool {
        self.inner.supports_tools(model)
    }

    fn supports_parallel_tool_config(&self, model: &str) -> bool {
        self.inner.supports_parallel_tool_config(model)
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let (provider, request) = self.prepare_request(request).await?;
        provider.generate(request).await
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let (provider, request) = self.prepare_request(request).await?;
        provider.stream(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        models::llamacpp::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted_error =
                error_display::format_llm_error("llama.cpp", "Messages cannot be empty");
            return Err(LLMError::InvalidRequest {
                message: formatted_error,
                metadata: None,
            });
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("openai") {
                let formatted = error_display::format_llm_error("llama.cpp", &err);
                return Err(LLMError::InvalidRequest {
                    message: formatted,
                    metadata: None,
                });
            }
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for LlamaCppProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        LLMProvider::generate(
            self,
            LLMRequest {
                messages: vec![Message::user(prompt.to_string())],
                model: self
                    .configured_model
                    .clone()
                    .unwrap_or_else(|| models::llamacpp::DEFAULT_MODEL.to_string()),
                ..Default::default()
            },
        )
        .await
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
}
