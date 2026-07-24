//! Shared state, registry, and readiness orchestration for managed llama.cpp
//! servers.
//!
//! Extracted verbatim from the original monolithic `llamacpp.rs`. Owns the
//! `ServerPhase` / `ServerStatus` state machine, the process-wide
//! `MANAGED_LLAMACPP_SERVERS` registry, and the `ensure_server_ready`
//! orchestration that reconciles probing, child reaping, and spawning.
//!
//! Uses `parking_lot::Mutex` for the registry so that a panic in one critical
//! section does not poison the lock and bring down every subsequent llama.cpp
//! operation. The per-server mutable state remains `tokio::sync::Mutex` because
//! it is held across `.await` points (child inspection, spawning, watch
//! subscription).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use parking_lot::Mutex;
use tokio::process::Child;
use tokio::sync::{Mutex as AsyncMutex, watch};

use crate::error_display;
use crate::provider::LLMError;
use crate::providers::local_server::is_local_base_url;
use crate::providers::ollama::base_url_to_host_root;

use vtcode_commons::llm::LLMErrorMetadata;
use vtcode_config::constants::env_vars;

use super::LLAMACPP_CONNECTION_ERROR;
use super::probe::{ServerProbe, probe_server};
use super::startup::{configured_startup_model_path, spawn_managed_server, startup_timeout, wait_until_ready};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ServerPhase {
    NotStarted,
    Starting,
    Ready,
    Failed,
}

#[derive(Debug, Clone)]
pub(super) struct ServerStatus {
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
pub(super) struct ManagedLlamaCppServer {
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
            state: AsyncMutex::new(ManagedLlamaCppState { child: None, status }),
            status_tx,
        }
    }
}

static MANAGED_LLAMACPP_SERVERS: LazyLock<Mutex<HashMap<String, Arc<ManagedLlamaCppServer>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn managed_server_for(base_url: &str) -> Arc<ManagedLlamaCppServer> {
    let host_root = base_url_to_host_root(base_url);
    // parking_lot::Mutex::lock never poisons, so no `.expect(...)` is needed
    // here (unlike the previous std::sync::Mutex implementation, which could
    // panic and cascade to every later llama.cpp operation).
    let mut guard = MANAGED_LLAMACPP_SERVERS.lock();
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

/// Reconcile probe state, child-process reaping, and (re)spawning until the
/// managed server reports a ready model id, or return an `LLMError` describing
/// why readiness could not be reached.
///
/// Moved substantially unchanged from the original `LlamaCppProvider::ensure_server_ready`
/// method; the only edits are `Self::` calls rewritten to submodule paths and
/// `self.base_url` / `self.configured_model` turned into parameters so this
/// can be a free function shared by the provider's request path.
pub(super) async fn ensure_server_ready(base_url: &str, configured_model: Option<&str>) -> Result<String, LLMError> {
    let timeout = startup_timeout();
    let initial_probe = probe_server(base_url).await;
    match &initial_probe {
        ServerProbe::Ready(model_id) => {
            let server = managed_server_for(base_url);
            let mut state = server.state.lock().await;
            state.status = ServerStatus::ready(model_id.clone(), state.status.model_path.clone());
            let _ = server.status_tx.send(state.status.clone());
            return Ok(model_id.clone());
        }
        ServerProbe::Loading => {}
        ServerProbe::Unavailable(_) => {}
    }

    let startup_model_path = configured_startup_model_path(configured_model);
    let server = managed_server_for(base_url);
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
                            format!("Managed llama.cpp server exited with status {exit_status}"),
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
                            if !is_local_base_url(base_url) {
                                return Err(provider_error(format!(
                                    "{LLAMACPP_CONNECTION_ERROR} Auto-start is only available for localhost llama.cpp endpoints."
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
                                    "llama.cpp is still loading but no managed model path is configured".to_string()
                                }
                                ServerProbe::Ready(model_id) => return Ok(model_id.clone()),
                            };
                            let message = format!(
                                "{reason} Set {} or configure the provider model to a local .gguf path so VT Code can launch llama-server automatically.",
                                env_vars::LLAMACPP_MODEL_PATH
                            );
                            // Tag with the unified local readiness code so the
                            // runloop can offer the /local recovery path.
                            return Err(LLMError::Provider {
                                message,
                                metadata: Some(LLMErrorMetadata::new(
                                    "llama.cpp",
                                    None,
                                    Some("local_server_down".to_string()),
                                    None,
                                    None,
                                    None,
                                    None,
                                )),
                            });
                        }
                    }
                }
            }
        }

        if should_spawn {
            let model_path = startup_model_path.clone().ok_or_else(|| {
                provider_error(format!(
                    "Managed llama.cpp startup requires {} or a provider model path",
                    env_vars::LLAMACPP_MODEL_PATH
                ))
            })?;

            let spawn_result = async {
                let child = spawn_managed_server(base_url, &model_path).await?;
                let model_id = wait_until_ready(base_url, timeout).await?;
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
                    return Err(provider_error(error.to_string()));
                }
            }
        }

        rx.changed()
            .await
            .map_err(|_e| provider_error("llama.cpp managed server watcher unexpectedly closed"))?;

        let status = rx.borrow().clone();
        match status.phase {
            ServerPhase::Ready => {
                if let Some(model_id) = status.model_id {
                    return Ok(model_id);
                }
            }
            ServerPhase::Failed => {
                return Err(provider_error(status.error.unwrap_or_else(|| LLAMACPP_CONNECTION_ERROR.to_string())));
            }
            ServerPhase::Starting | ServerPhase::NotStarted => {}
        }
    }
}
