//! Unified readiness pre-flight for local inference providers.
//!
//! Local servers (Ollama, LM Studio, llama.cpp) are frequently stopped or have
//! no model loaded. Previously generation simply failed with a raw connection
//! or 404 error. This module centralizes the "is the server up?" and "is the
//! requested model available?" checks so every local provider can return a
//! single, actionable error (with the exact fix command) instead of a cryptic
//! one. It also resolves a placeholder/default request model to the single
//! loaded model when appropriate.
//!
//! Design notes:
//! - Cloud Ollama models (`:cloud` / `-cloud`) are remote and bypass readiness.
//! - Results are cached per-process for a short TTL so we do not probe the
//!   local server on every single generation, while still allowing a
//!   `ServerDown` error to clear quickly after the user starts the server.
//! - This module is intentionally provider-agnostic: it only consumes the
//!   `LocalProvider` enum and the per-provider `fetch_*_models` helpers.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use vtcode_commons::llm::LLMError;

use super::llamacpp::fetch_llamacpp_models;
use super::lmstudio::fetch_lmstudio_models;
use super::local_server::{LocalProvider, probe};
use super::ollama::fetch_ollama_models;

const READINESS_CACHE_TTL: Duration = Duration::from_secs(15);

struct CacheEntry {
    verified_at: Instant,
    models: Vec<String>,
}

static READINESS_CACHE: LazyLock<Mutex<HashMap<LocalProvider, CacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Failure modes surfaced before a generation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalReadinessError {
    /// The local server process is not reachable.
    ServerDown { provider: LocalProvider },
    /// The server is up but the requested model is not available/loaded.
    ModelMissing { provider: LocalProvider, model: String },
}

impl LocalReadinessError {
    /// Stable tag stored in `LLMErrorMetadata.code` so the runloop can render
    /// an interactive "start server" / "pull model" offer.
    fn code(&self) -> &'static str {
        match self {
            Self::ServerDown { .. } => "local_server_down",
            Self::ModelMissing { .. } => "local_model_missing",
        }
    }

    /// The exact command/instruction the user needs to run to recover.
    fn fix_command(&self) -> String {
        match self {
            Self::ServerDown { provider } => format!("/local start {}", provider.key()),
            Self::ModelMissing { provider, model } => match provider {
                LocalProvider::Ollama => format!("ollama pull {model}"),
                LocalProvider::LmStudio => format!("lms load {model}"),
                LocalProvider::LlamaCpp => {
                    format!("load '{model}' in llama.cpp (set LLAMACPP_MODEL_PATH and run /local start llamacpp)")
                }
            },
        }
    }

    /// Human-readable recovery instruction (used in logs/troubleshooting).
    fn recovery_hint(&self) -> String {
        match self {
            Self::ServerDown { provider } => format!(
                "{} server is not running. Start it with `/local start {}` (or the app/CLI), \
                 then retry.",
                provider.display_name(),
                provider.key()
            ),
            Self::ModelMissing { provider, model } => {
                format!("Model '{model}' is not available on {}. Fix: {}", provider.display_name(), self.fix_command())
            }
        }
    }

    /// Convert into a structured `LLMError` carrying the recovery code so the
    /// runloop can offer the user a one-tap fix.
    pub(crate) fn to_llm_error(&self, display: &str) -> LLMError {
        LLMError::Provider {
            message: self.recovery_hint(),
            metadata: Some(vtcode_commons::llm::LLMErrorMetadata::new(
                display,
                None,
                Some(self.code().to_string()),
                None,
                None,
                None,
                None,
            )),
        }
    }
}

fn is_cloud_ollama_model(model: &str) -> bool {
    model.contains(":cloud") || model.contains("-cloud")
}

/// Resolve the model that should actually be used for the request.
///
/// Returns the (possibly substituted) model id, or a [`LocalReadinessError`]
/// describing what is wrong. An explicit, non-empty `requested` model that is
/// not available is always reported as missing (with the recovery command) —
/// we never silently swap an explicit selection for a different loaded model.
/// Only when *no* model is specified (empty request) do we fall back to the
/// single loaded model.
pub(crate) async fn resolve_local_model(
    provider: LocalProvider,
    requested: &str,
    base_url: Option<&str>,
) -> Result<String, LocalReadinessError> {
    if provider == LocalProvider::Ollama && is_cloud_ollama_model(requested) {
        return Ok(requested.to_string());
    }

    let status = probe(provider).await;
    if !status.running {
        return Err(LocalReadinessError::ServerDown { provider });
    }

    let models = cached_models(provider, base_url).await;

    match models {
        Some(list) if !list.is_empty() => {
            if list.iter().any(|m| m == requested) {
                return Ok(requested.to_string());
            }
            if requested.trim().is_empty() && list.len() == 1 {
                return Ok(list[0].clone());
            }
            Err(LocalReadinessError::ModelMissing { provider, model: requested.to_string() })
        }
        // Server is up but we could not enumerate models: trust the request id
        // rather than blocking the user with a false negative.
        _ => Ok(requested.to_string()),
    }
}

async fn cached_models(provider: LocalProvider, base_url: Option<&str>) -> Option<Vec<String>> {
    {
        let guard = READINESS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get(&provider)
            && entry.verified_at.elapsed() < READINESS_CACHE_TTL
        {
            return Some(entry.models.clone());
        }
    }

    let fetched = fetch_models(provider, base_url).await;
    if let Ok(models) = fetched {
        if let Ok(mut guard) = READINESS_CACHE.lock() {
            guard.insert(
                provider,
                CacheEntry {
                    verified_at: Instant::now(),
                    models: models.clone(),
                },
            );
        }
        Some(models)
    } else {
        None
    }
}

async fn fetch_models(provider: LocalProvider, base_url: Option<&str>) -> anyhow::Result<Vec<String>> {
    let base = base_url.map(str::to_string);
    match provider {
        LocalProvider::Ollama => fetch_ollama_models(base).await,
        LocalProvider::LmStudio => fetch_lmstudio_models(base).await,
        LocalProvider::LlamaCpp => fetch_llamacpp_models(base).await,
    }
}

/// Clear the cached readiness state (used by tests and after a server lifecycle
/// change so a subsequent generation re-probes immediately).
pub(crate) fn invalidate_readiness_cache() {
    if let Ok(mut guard) = READINESS_CACHE.lock() {
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fix_command_for_server_down() {
        let err = LocalReadinessError::ServerDown { provider: LocalProvider::Ollama };
        assert_eq!(err.code(), "local_server_down");
        assert_eq!(err.fix_command(), "/local start ollama");
    }

    #[test]
    fn fix_command_for_missing_model() {
        let ollama = LocalReadinessError::ModelMissing {
            provider: LocalProvider::Ollama,
            model: "gpt-oss:20b".to_string(),
        };
        assert_eq!(ollama.fix_command(), "ollama pull gpt-oss:20b");
        assert_eq!(ollama.code(), "local_model_missing");

        let lm = LocalReadinessError::ModelMissing {
            provider: LocalProvider::LmStudio,
            model: "my-model".to_string(),
        };
        assert_eq!(lm.fix_command(), "lms load my-model");
    }

    #[test]
    fn cloud_ollama_models_bypass_check() {
        assert!(is_cloud_ollama_model("deepseek-v4-flash:cloud"));
        assert!(is_cloud_ollama_model("glm-5.2-cloud"));
        assert!(!is_cloud_ollama_model("gpt-oss:20b"));
    }
}
