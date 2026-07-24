//! Model-path policy, binary resolution, process spawning, and readiness
//! polling for managed llama.cpp servers.
//!
//! Extracted verbatim from the original monolithic `llamacpp.rs`. These are
//! the pure / I/O seams around launching a local `llama-server` process and
//! waiting for it to become ready. Shared state and orchestration live in
//! `managed.rs`.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::process::{Child, Command};
use tokio::time::sleep;
use url::Url;

use crate::providers::ollama::base_url_to_host_root;

use vtcode_config::constants::env_vars;

use super::LLAMACPP_CONNECTION_ERROR;
use super::probe::{ServerProbe, probe_server};

const DEFAULT_STARTUP_TIMEOUT_SECONDS: u64 = 60;
const SERVER_POLL_INTERVAL: Duration = Duration::from_millis(500);

pub(super) fn configured_startup_model_path(configured_model: Option<&str>) -> Option<String> {
    std::env::var(env_vars::LLAMACPP_MODEL_PATH)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            configured_model.and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() || !super::looks_like_local_model_path(trimmed) {
                    return None;
                }
                Some(trimmed.to_string())
            })
        })
}

pub(super) fn startup_timeout() -> Duration {
    std::env::var(env_vars::LLAMACPP_STARTUP_TIMEOUT_SECONDS)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_STARTUP_TIMEOUT_SECONDS))
}

pub(super) fn host_port(base_url: &str) -> Result<u16> {
    let host_root = base_url_to_host_root(base_url);
    let parsed = Url::parse(&host_root).with_context(|| format!("Failed to parse llama.cpp base URL: {host_root}"))?;
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

pub(super) fn build_command_args(base_url: &str, model_path: &str) -> Result<Vec<String>> {
    let path = Path::new(model_path);
    if !path.exists() {
        anyhow::bail!("Configured model path does not exist: {model_path}");
    }

    let mut args = vec![
        "-m".to_string(),
        model_path.to_string(),
        "--port".to_string(),
        host_port(base_url)?.to_string(),
    ];

    if let Ok(extra_args) = std::env::var(env_vars::LLAMACPP_EXTRA_ARGS)
        && !extra_args.trim().is_empty()
    {
        args.extend(
            shell_words::split(&extra_args)
                .with_context(|| format!("Failed to parse {}: {extra_args}", env_vars::LLAMACPP_EXTRA_ARGS))?,
        );
    }

    Ok(args)
}

pub(super) async fn spawn_managed_server(base_url: &str, model_path: &str) -> Result<Child> {
    let binary = resolve_binary_path()?;
    let args = build_command_args(base_url, model_path)?;
    let mut command = Command::new(&binary);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    command
        .spawn()
        .with_context(|| format!("Failed to start llama.cpp server with `{binary} {}`", args.join(" ")))
}

pub(super) async fn wait_until_ready(base_url: &str, timeout: Duration) -> Result<String> {
    let deadline = Instant::now() + timeout;
    let mut last_error = LLAMACPP_CONNECTION_ERROR.to_string();

    while Instant::now() < deadline {
        match probe_server(base_url).await {
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

#[cfg(test)]
mod tests {
    use super::{build_command_args, host_port};

    #[test]
    fn host_port_extracts_explicit_port() {
        assert_eq!(host_port("http://localhost:1234/v1").unwrap(), 1234);
        assert_eq!(host_port("http://127.0.0.1:8088").unwrap(), 8088);
    }

    #[test]
    fn host_port_defaults_to_8080_when_absent() {
        assert_eq!(host_port("http://localhost/v1").unwrap(), 8080);
    }

    #[test]
    fn host_port_rejects_invalid_url() {
        assert!(host_port("not a url").is_err());
    }

    #[test]
    fn build_command_args_includes_model_and_port() {
        // Use a path that is guaranteed to exist: this crate's Cargo.toml.
        let model_path = format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
        let args = build_command_args("http://localhost:9090/v1", &model_path).unwrap();

        assert_eq!(args[0], "-m");
        assert_eq!(args[1], model_path);
        assert_eq!(args[2], "--port");
        assert_eq!(args[3], "9090");
    }

    #[test]
    fn build_command_args_rejects_missing_model() {
        assert!(build_command_args("http://localhost:9090/v1", "/does/not/exist.gguf").is_err());
    }
}
