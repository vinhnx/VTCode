use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::process::Child;

use vtcode_config::constants::{env_vars, urls};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalProvider {
    Ollama,
    LmStudio,
    LlamaCpp,
}

impl LocalProvider {
    pub fn key(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::LmStudio => "lmstudio",
            Self::LlamaCpp => "llamacpp",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Ollama => "Ollama",
            Self::LmStudio => "LM Studio",
            Self::LlamaCpp => "llama.cpp",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "ollama" => Some(Self::Ollama),
            "lmstudio" | "lm-studio" => Some(Self::LmStudio),
            "llamacpp" | "llama.cpp" | "llama-cpp" => Some(Self::LlamaCpp),
            _ => None,
        }
    }

    pub fn all() -> &'static [LocalProvider] {
        &[Self::Ollama, Self::LmStudio, Self::LlamaCpp]
    }

    fn default_port(self) -> u16 {
        match self {
            Self::Ollama => 11434,
            Self::LmStudio => 1234,
            Self::LlamaCpp => 8080,
        }
    }

    fn default_base_url(self) -> &'static str {
        match self {
            Self::Ollama => urls::OLLAMA_API_BASE,
            Self::LmStudio => urls::LMSTUDIO_API_BASE,
            Self::LlamaCpp => urls::LLAMACPP_API_BASE,
        }
    }

    fn base_url_env(self) -> &'static str {
        match self {
            Self::Ollama => env_vars::OLLAMA_BASE_URL,
            Self::LmStudio => env_vars::LMSTUDIO_BASE_URL,
            Self::LlamaCpp => env_vars::LLAMACPP_BASE_URL,
        }
    }

    pub fn base_url(self) -> String {
        resolve_base_url(self.default_base_url(), self.base_url_env())
    }

    fn host_root(self) -> String {
        let base = self.base_url();
        strip_path_suffix(&base)
    }
}

#[derive(Debug, Clone)]
pub struct LocalServerStatus {
    pub provider: LocalProvider,
    pub running: bool,
    pub endpoint: String,
    pub available_models: Vec<String>,
    pub running_models: Vec<String>,
    pub version: Option<String>,
    pub error: Option<String>,
}

impl LocalServerStatus {
    pub fn not_running(provider: LocalProvider, reason: impl Into<String>) -> Self {
        Self {
            provider,
            running: false,
            endpoint: provider.base_url(),
            available_models: Vec::new(),
            running_models: Vec::new(),
            version: None,
            error: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LocalServerCapabilities {
    pub can_start: bool,
    pub can_stop: bool,
    pub binary_found: bool,
    pub binary_name: &'static str,
    pub binary_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EnvVarInfo {
    pub name: &'static str,
    pub current_value: Option<String>,
    pub description: &'static str,
}

// ---------------------------------------------------------------------------
// Managed process tracking (for Ollama and llama.cpp)
// ---------------------------------------------------------------------------

struct ManagedProcess {
    child: Option<Child>,
}

static MANAGED_PROCESSES: LazyLock<Mutex<HashMap<LocalProvider, ManagedProcess>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn take_managed_child(provider: LocalProvider) -> Option<Child> {
    let mut guard = MANAGED_PROCESSES.lock().ok()?;
    guard.get_mut(&provider)?.child.take()
}

fn store_managed_child(provider: LocalProvider, child: Child) {
    if let Ok(mut guard) = MANAGED_PROCESSES.lock() {
        guard.entry(provider).or_insert_with(|| ManagedProcess { child: None }).child = Some(child);
    }
}

fn is_managed_running(provider: LocalProvider) -> bool {
    MANAGED_PROCESSES
        .lock()
        .ok()
        .and_then(|guard| guard.get(&provider).map(|p| p.child.is_some()))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub async fn probe_all() -> Vec<LocalServerStatus> {
    let mut statuses = Vec::with_capacity(LocalProvider::all().len());
    for &provider in LocalProvider::all() {
        statuses.push(probe(provider).await);
    }
    statuses
}

pub async fn probe(provider: LocalProvider) -> LocalServerStatus {
    match provider {
        LocalProvider::Ollama => probe_ollama().await,
        LocalProvider::LmStudio => probe_lmstudio().await,
        LocalProvider::LlamaCpp => probe_llamacpp().await,
    }
}

pub async fn start(provider: LocalProvider) -> Result<String> {
    match provider {
        LocalProvider::Ollama => start_ollama().await,
        LocalProvider::LmStudio => start_lmstudio().await,
        LocalProvider::LlamaCpp => start_llamacpp().await,
    }
}

pub async fn stop(provider: LocalProvider) -> Result<String> {
    match provider {
        LocalProvider::Ollama => stop_ollama().await,
        LocalProvider::LmStudio => stop_lmstudio().await,
        LocalProvider::LlamaCpp => stop_llamacpp().await,
    }
}

pub fn capabilities(provider: LocalProvider) -> LocalServerCapabilities {
    match provider {
        LocalProvider::Ollama => caps_ollama(),
        LocalProvider::LmStudio => caps_lmstudio(),
        LocalProvider::LlamaCpp => caps_llamacpp(),
    }
}

pub fn env_config(provider: LocalProvider) -> Vec<EnvVarInfo> {
    match provider {
        LocalProvider::Ollama => vec![EnvVarInfo {
            name: env_vars::OLLAMA_BASE_URL,
            current_value: std::env::var(env_vars::OLLAMA_BASE_URL).ok(),
            description: "Ollama server base URL (default: http://localhost:11434)",
        }],
        LocalProvider::LmStudio => vec![EnvVarInfo {
            name: env_vars::LMSTUDIO_BASE_URL,
            current_value: std::env::var(env_vars::LMSTUDIO_BASE_URL).ok(),
            description: "LM Studio server base URL (default: http://localhost:1234/v1)",
        }],
        LocalProvider::LlamaCpp => vec![
            EnvVarInfo {
                name: env_vars::LLAMACPP_BASE_URL,
                current_value: std::env::var(env_vars::LLAMACPP_BASE_URL).ok(),
                description: "llama.cpp server base URL (default: http://localhost:8080/v1)",
            },
            EnvVarInfo {
                name: env_vars::LLAMACPP_MODEL_PATH,
                current_value: std::env::var(env_vars::LLAMACPP_MODEL_PATH).ok(),
                description: "Path to .gguf model file for auto-start",
            },
            EnvVarInfo {
                name: env_vars::LLAMACPP_BINARY_PATH,
                current_value: std::env::var(env_vars::LLAMACPP_BINARY_PATH).ok(),
                description: "Path to llama-server binary (default: search PATH)",
            },
            EnvVarInfo {
                name: env_vars::LLAMACPP_EXTRA_ARGS,
                current_value: std::env::var(env_vars::LLAMACPP_EXTRA_ARGS).ok(),
                description: "Extra arguments passed to llama-server",
            },
        ],
    }
}

pub fn troubleshoot(status: &LocalServerStatus, caps: &LocalServerCapabilities) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("{} Troubleshoot", status.provider.display_name()));
    lines.push(String::new());

    if status.running {
        lines.push("Server is running and responding.".to_string());
        if status.available_models.is_empty() {
            lines.push("No models are currently available.".to_string());
            match status.provider {
                LocalProvider::Ollama => {
                    lines.push("  Pull a model: ollama pull gemma3".to_string());
                }
                LocalProvider::LmStudio => {
                    lines.push(
                        "  Download a model in LM Studio or run: lms get <model>".to_string(),
                    );
                }
                LocalProvider::LlamaCpp => {
                    lines.push(format!(
                        "  Set {}=/path/to/model.gguf and restart",
                        env_vars::LLAMACPP_MODEL_PATH
                    ));
                }
            }
        }
        return lines;
    }

    lines.push("Status: Not running".to_string());
    if let Some(err) = &status.error {
        lines.push(format!("Error: {err}"));
    }
    lines.push(String::new());

    match status.provider {
        LocalProvider::Ollama => {
            if !caps.binary_found {
                lines.push("Ollama is not installed.".to_string());
                lines.push("  Install: brew install ollama".to_string());
                lines.push("  Or: https://github.com/ollama/ollama?tab=readme-ov-file".to_string());
            } else {
                lines.push("Ollama is installed but the server is not running.".to_string());
                lines.push("  Start: ollama serve".to_string());
                lines.push("  Or: /local start ollama".to_string());
            }
            lines.push("  Logs: ~/.ollama/logs/server.log".to_string());
        }
        LocalProvider::LmStudio => {
            if !caps.binary_found {
                lines.push("LM Studio CLI (lms) not found.".to_string());
                lines.push("  Install LM Studio: https://lmstudio.ai/download".to_string());
                lines.push("  The lms CLI ships with LM Studio.".to_string());
            } else {
                lines.push("LM Studio server is not running.".to_string());
                lines.push("  Start: lms server start".to_string());
                lines.push("  Or: /local start lmstudio".to_string());
                lines.push("  Status: lms server status --json".to_string());
            }
        }
        LocalProvider::LlamaCpp => {
            if !caps.binary_found {
                lines.push("llama-server binary not found.".to_string());
                lines.push("  Install: https://llama.app".to_string());
                lines.push(format!(
                    "  Or set {}=/path/to/llama-server",
                    env_vars::LLAMACPP_BINARY_PATH
                ));
            } else {
                lines.push("llama.cpp server is not running.".to_string());
                let model_path = std::env::var(env_vars::LLAMACPP_MODEL_PATH).ok();
                if model_path.is_none() {
                    lines.push(format!(
                        "  Set {}=/path/to/model.gguf for auto-start",
                        env_vars::LLAMACPP_MODEL_PATH
                    ));
                }
                lines.push("  Or: /local start llamacpp".to_string());
            }
        }
    }

    lines
}

// ---------------------------------------------------------------------------
// Probe implementations
// ---------------------------------------------------------------------------

async fn probe_ollama() -> LocalServerStatus {
    let base = LocalProvider::Ollama.host_root();
    let client = vtcode_commons::http::create_client_with_timeout(PROBE_TIMEOUT);

    // Check /api/tags for availability + models
    let tags_url = format!("{}/api/tags", base.trim_end_matches('/'));
    let tags_resp = match client.get(&tags_url).send().await {
        Ok(resp) => resp,
        Err(e) => {
            let mut s = LocalServerStatus::not_running(LocalProvider::Ollama, e.to_string());
            if is_managed_running(LocalProvider::Ollama) {
                s.error = Some("Managed process exists but server not responding yet".into());
            }
            return s;
        }
    };

    if !tags_resp.status().is_success() {
        return LocalServerStatus::not_running(
            LocalProvider::Ollama,
            format!("HTTP {}", tags_resp.status()),
        );
    }

    let available_models = tags_resp
        .json::<OllamaTagsResponse>()
        .await
        .map(|r| r.models.into_iter().map(|m| m.name).collect())
        .unwrap_or_default();

    // Check /api/ps for running models
    let ps_url = format!("{}/api/ps", base.trim_end_matches('/'));
    let running_models = parse_json_opt::<OllamaPsResponse>(client.get(&ps_url).send().await.ok())
        .await
        .map(|r| r.models.into_iter().map(|m| m.name).collect())
        .unwrap_or_default();

    // Check /api/version
    let version_url = format!("{}/api/version", base.trim_end_matches('/'));
    let version =
        parse_json_opt::<OllamaVersionResponse>(client.get(&version_url).send().await.ok())
            .await
            .and_then(|r| r.version);

    LocalServerStatus {
        provider: LocalProvider::Ollama,
        running: true,
        endpoint: LocalProvider::Ollama.base_url(),
        available_models,
        running_models,
        version,
        error: None,
    }
}

async fn probe_lmstudio() -> LocalServerStatus {
    let base = LocalProvider::LmStudio.base_url();
    let client = vtcode_commons::http::create_client_with_timeout(PROBE_TIMEOUT);

    let models_url = format!("{}/models", base.trim_end_matches('/'));
    let resp = match client.get(&models_url).send().await {
        Ok(resp) => resp,
        Err(e) => return LocalServerStatus::not_running(LocalProvider::LmStudio, e.to_string()),
    };

    if !resp.status().is_success() {
        return LocalServerStatus::not_running(
            LocalProvider::LmStudio,
            format!("HTTP {}", resp.status()),
        );
    }

    let available_models = resp
        .json::<LmStudioModelsResponse>()
        .await
        .map(|r| r.data.into_iter().map(|m| m.id).collect())
        .unwrap_or_default();

    LocalServerStatus {
        provider: LocalProvider::LmStudio,
        running: true,
        endpoint: LocalProvider::LmStudio.base_url(),
        available_models,
        running_models: Vec::new(),
        version: None,
        error: None,
    }
}

async fn probe_llamacpp() -> LocalServerStatus {
    let base = LocalProvider::LlamaCpp.host_root();
    let client = vtcode_commons::http::create_client_with_timeout(PROBE_TIMEOUT);

    // Check /health
    let health_url = format!("{}/health", base.trim_end_matches('/'));
    let health_resp = match client.get(&health_url).send().await {
        Ok(resp) => resp,
        Err(e) => return LocalServerStatus::not_running(LocalProvider::LlamaCpp, e.to_string()),
    };

    if !health_resp.status().is_success() {
        return LocalServerStatus::not_running(
            LocalProvider::LlamaCpp,
            format!("HTTP {}", health_resp.status()),
        );
    }

    // Check /models
    let models_url = format!("{}/models", base.trim_end_matches('/'));
    let available_models =
        parse_json_opt::<LlamaCppModelsResponse>(client.get(&models_url).send().await.ok())
            .await
            .map(|r| r.data.into_iter().map(|m| m.id).collect())
            .unwrap_or_default();

    LocalServerStatus {
        provider: LocalProvider::LlamaCpp,
        running: true,
        endpoint: LocalProvider::LlamaCpp.base_url(),
        available_models,
        running_models: Vec::new(),
        version: None,
        error: None,
    }
}

// ---------------------------------------------------------------------------
// Start implementations
// ---------------------------------------------------------------------------

async fn start_ollama() -> Result<String> {
    let caps = caps_ollama();
    if !caps.binary_found {
        anyhow::bail!(
            "Ollama is not installed. Install with: brew install ollama\n\
             Or visit: https://github.com/ollama/ollama?tab=readme-ov-file"
        );
    }

    // Check if already running
    let status = probe_ollama().await;
    if status.running {
        return Ok("Ollama is already running.".to_string());
    }

    let binary = caps.binary_path.unwrap_or_else(|| "ollama".to_string());
    let mut cmd = tokio::process::Command::new(&binary);
    cmd.arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let child = cmd
        .spawn()
        .with_context(|| format!("Failed to start Ollama with `{binary} serve`"))?;

    store_managed_child(LocalProvider::Ollama, child);

    // Wait for it to become ready
    wait_for_ready(LocalProvider::Ollama, Duration::from_secs(10)).await?;

    Ok("Ollama server started.".to_string())
}

async fn start_lmstudio() -> Result<String> {
    let caps = caps_lmstudio();
    if !caps.binary_found {
        anyhow::bail!(
            "LM Studio CLI (lms) not found.\n\
             Install LM Studio from https://lmstudio.ai/download\n\
             The lms CLI ships with the app."
        );
    }

    // Check if already running
    let status = probe_lmstudio().await;
    if status.running {
        return Ok("LM Studio server is already running.".to_string());
    }

    let binary = caps.binary_path.unwrap_or_else(|| "lms".to_string());
    let output = tokio::process::Command::new(&binary)
        .args(["server", "start"])
        .output()
        .await
        .with_context(|| format!("Failed to run `{binary} server start`"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("lms server start failed: {}", stderr.trim());
    }

    // Wait for it to become ready
    wait_for_ready(LocalProvider::LmStudio, Duration::from_secs(10)).await?;

    Ok("LM Studio server started.".to_string())
}

async fn start_llamacpp() -> Result<String> {
    let caps = caps_llamacpp();
    if !caps.binary_found {
        anyhow::bail!(
            "llama-server binary not found.\n\
             Install from https://llama.app\n\
             Or set {}=/path/to/llama-server",
            env_vars::LLAMACPP_BINARY_PATH
        );
    }

    // Check if already running
    let status = probe_llamacpp().await;
    if status.running {
        return Ok("llama.cpp server is already running.".to_string());
    }

    let model_path = std::env::var(env_vars::LLAMACPP_MODEL_PATH)
        .ok()
        .filter(|v| !v.trim().is_empty());
    let model_path = match model_path {
        Some(path) => path,
        None => anyhow::bail!(
            "Set {}=/path/to/model.gguf to enable auto-start for llama.cpp",
            env_vars::LLAMACPP_MODEL_PATH
        ),
    };

    let binary = caps.binary_path.unwrap_or_else(|| "llama-server".to_string());
    let port = extract_port(&LocalProvider::LlamaCpp.base_url())
        .unwrap_or(LocalProvider::LlamaCpp.default_port());

    let mut args = vec![
        "-m".to_string(),
        model_path,
        "--port".to_string(),
        port.to_string(),
    ];
    if let Ok(extra) = std::env::var(env_vars::LLAMACPP_EXTRA_ARGS)
        && !extra.trim().is_empty()
    {
        args.extend(shell_words::split(&extra).unwrap_or_default());
    }

    let mut cmd = tokio::process::Command::new(&binary);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let child = cmd.spawn().with_context(|| {
        format!("Failed to start llama-server (`{binary} -m <model> --port {port}`)")
    })?;

    store_managed_child(LocalProvider::LlamaCpp, child);

    // Wait for it to become ready
    wait_for_ready(LocalProvider::LlamaCpp, Duration::from_secs(30)).await?;

    Ok("llama.cpp server started.".to_string())
}

// ---------------------------------------------------------------------------
// Stop implementations
// ---------------------------------------------------------------------------

async fn stop_ollama() -> Result<String> {
    if let Some(mut child) = take_managed_child(LocalProvider::Ollama) {
        child.kill().await.ok();
        return Ok("Ollama server stopped.".to_string());
    }

    // No managed process; check if it's running externally
    let status = probe_ollama().await;
    if !status.running {
        return Ok("Ollama is not running.".to_string());
    }

    anyhow::bail!(
        "Ollama is running but was not started by VT Code.\n\
         Stop it manually or kill the process."
    )
}

async fn stop_lmstudio() -> Result<String> {
    let caps = caps_lmstudio();
    if !caps.binary_found {
        return Ok("LM Studio CLI not found; nothing to stop.".to_string());
    }

    let status = probe_lmstudio().await;
    if !status.running {
        return Ok("LM Studio server is not running.".to_string());
    }

    let binary = caps.binary_path.unwrap_or_else(|| "lms".to_string());
    let output = tokio::process::Command::new(&binary)
        .args(["server", "stop"])
        .output()
        .await
        .with_context(|| format!("Failed to run `{binary} server stop`"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("lms server stop failed: {}", stderr.trim());
    }

    Ok("LM Studio server stopped.".to_string())
}

async fn stop_llamacpp() -> Result<String> {
    // Try our own managed child first
    if let Some(mut child) = take_managed_child(LocalProvider::LlamaCpp) {
        child.kill().await.ok();
        return Ok("llama.cpp server stopped.".to_string());
    }

    // Check if running externally
    let status = probe_llamacpp().await;
    if !status.running {
        return Ok("llama.cpp server is not running.".to_string());
    }

    anyhow::bail!(
        "llama.cpp is running but was not started by VT Code.\n\
         Stop it manually or kill the process."
    )
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

fn caps_ollama() -> LocalServerCapabilities {
    let (found, path) = find_binary("ollama");
    LocalServerCapabilities {
        can_start: found,
        can_stop: is_managed_running(LocalProvider::Ollama),
        binary_found: found,
        binary_name: "ollama",
        binary_path: path,
    }
}

fn caps_lmstudio() -> LocalServerCapabilities {
    // Try `lms` on PATH, then fallback to ~/.lmstudio/bin/lms
    let (found, path) = find_binary("lms");
    let (found, path) = if !found {
        find_lms_fallback().unwrap_or((false, None))
    } else {
        (found, path)
    };
    LocalServerCapabilities {
        can_start: found,
        can_stop: found,
        binary_found: found,
        binary_name: "lms",
        binary_path: path,
    }
}

fn caps_llamacpp() -> LocalServerCapabilities {
    // Check LLAMACPP_BINARY_PATH first, then PATH
    let (found, path) = if let Ok(explicit) = std::env::var(env_vars::LLAMACPP_BINARY_PATH) {
        if !explicit.trim().is_empty() && std::path::Path::new(explicit.trim()).exists() {
            (true, Some(explicit.trim().to_string()))
        } else {
            find_binary("llama-server")
        }
    } else {
        find_binary("llama-server")
    };

    LocalServerCapabilities {
        can_start: found
            && std::env::var(env_vars::LLAMACPP_MODEL_PATH)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .is_some(),
        can_stop: is_managed_running(LocalProvider::LlamaCpp),
        binary_found: found,
        binary_name: "llama-server",
        binary_path: path,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_base_url(default: &str, env_var: &str) -> String {
    std::env::var(env_var)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Returns true when `base_url` points at a loopback/local endpoint.
///
/// Shared by local providers so the "is this localhost?" check lives in one
/// place. Accepts `localhost`, `127.0.0.1` (and the whole `127.0.0.0/8`
/// subnet via prefix match), `::1`, and `0.0.0.0` over http/https.
pub fn is_local_base_url(base_url: &str) -> bool {
    let lowered = base_url.trim().to_ascii_lowercase();
    const LOCAL_PREFIXES: &[&str] = &[
        "http://localhost",
        "https://localhost",
        "http://127.",
        "https://127.",
        "http://0.0.0.0",
        "https://0.0.0.0",
        "http://[::1]",
        "https://[::1]",
    ];
    if LOCAL_PREFIXES.iter().any(|prefix| lowered.starts_with(*prefix)) {
        return true;
    }
    if let Ok(parsed) = url::Url::parse(lowered.trim_end_matches('/'))
        && let Some(host) = parsed.host_str()
    {
        return matches!(host, "localhost" | "127.0.0.1" | "::1" | "0.0.0.0");
    }
    false
}

fn strip_path_suffix(url: &str) -> String {
    // Strip /v1 or /api/v1 suffix to get the host root
    let trimmed = url.trim_end_matches('/');
    if let Some(pos) = trimmed.rfind("/v1") {
        trimmed[..pos].to_string()
    } else {
        trimmed.to_string()
    }
}

fn extract_port(url: &str) -> Option<u16> {
    let stripped = strip_path_suffix(url);
    url::Url::parse(&stripped).ok().and_then(|u| u.port())
}

fn find_binary(name: &str) -> (bool, Option<String>) {
    which::which(name)
        .map(|p| (true, Some(p.to_string_lossy().into_owned())))
        .unwrap_or((false, None))
}

fn find_lms_fallback() -> Option<(bool, Option<String>)> {
    let home = std::env::var("HOME").ok()?;
    let fallback = format!("{home}/.lmstudio/bin/lms");
    if std::path::Path::new(&fallback).exists() {
        Some((true, Some(fallback)))
    } else {
        None
    }
}

async fn wait_for_ready(provider: LocalProvider, timeout: Duration) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut last_error = String::new();

    while tokio::time::Instant::now() < deadline {
        let status = probe(provider).await;
        if status.running {
            return Ok(());
        }
        if let Some(err) = &status.error {
            last_error = err.clone();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    anyhow::bail!(
        "Timed out waiting for {} to start after {}s. Last: {}",
        provider.display_name(),
        timeout.as_secs(),
        last_error
    )
}

// Response types

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelSummary>,
}

#[derive(Deserialize)]
struct OllamaModelSummary {
    name: String,
}

#[derive(Deserialize)]
struct OllamaPsResponse {
    models: Vec<OllamaRunningModel>,
}

#[derive(Deserialize)]
struct OllamaRunningModel {
    name: String,
}

#[derive(Deserialize)]
struct OllamaVersionResponse {
    version: Option<String>,
}

#[derive(Deserialize)]
struct LmStudioModelsResponse {
    data: Vec<LmStudioModel>,
}

#[derive(Deserialize)]
struct LmStudioModel {
    id: String,
}

#[derive(Deserialize)]
struct LlamaCppModelsResponse {
    data: Vec<LlamaCppModel>,
}

#[derive(Deserialize)]
struct LlamaCppModel {
    id: String,
}

// Helper: parse response as JSON, returning None on failure
async fn parse_json_opt<T: serde::de::DeserializeOwned>(
    resp: Option<reqwest::Response>,
) -> Option<T> {
    let resp = resp?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<T>().await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_from_key() {
        assert_eq!(LocalProvider::from_key("ollama"), Some(LocalProvider::Ollama));
        assert_eq!(LocalProvider::from_key("lmstudio"), Some(LocalProvider::LmStudio));
        assert_eq!(LocalProvider::from_key("lm-studio"), Some(LocalProvider::LmStudio));
        assert_eq!(LocalProvider::from_key("llamacpp"), Some(LocalProvider::LlamaCpp));
        assert_eq!(LocalProvider::from_key("llama.cpp"), Some(LocalProvider::LlamaCpp));
        assert_eq!(LocalProvider::from_key("unknown"), None);
    }

    #[test]
    fn test_provider_key_roundtrip() {
        for &p in LocalProvider::all() {
            assert_eq!(LocalProvider::from_key(p.key()), Some(p));
        }
    }

    #[test]
    fn test_provider_display_names() {
        assert_eq!(LocalProvider::Ollama.display_name(), "Ollama");
        assert_eq!(LocalProvider::LmStudio.display_name(), "LM Studio");
        assert_eq!(LocalProvider::LlamaCpp.display_name(), "llama.cpp");
    }

    #[test]
    fn test_strip_path_suffix() {
        assert_eq!(strip_path_suffix("http://localhost:11434/v1"), "http://localhost:11434");
        assert_eq!(strip_path_suffix("http://localhost:1234/v1/"), "http://localhost:1234");
        assert_eq!(strip_path_suffix("http://localhost:8080"), "http://localhost:8080");
    }

    #[test]
    fn test_is_local_base_url_accepts_loopback() {
        for url in [
            "http://localhost:11434",
            "https://localhost:1234/v1",
            "http://127.0.0.1:8080/v1",
            "http://127.1.2.3:9999",
            "http://0.0.0.0:8080",
            "http://[::1]:1234/v1",
        ] {
            assert!(is_local_base_url(url), "expected local: {url}");
        }
    }

    #[test]
    fn test_is_local_base_url_rejects_remote() {
        for url in [
            "http://192.168.1.10:11434",
            "https://api.openai.com/v1",
            "http://example.com:8080/v1",
            "http://10.0.0.5:1234",
        ] {
            assert!(!is_local_base_url(url), "expected remote: {url}");
        }
    }
}
