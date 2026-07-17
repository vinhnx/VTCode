use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Root state for VT Code pod management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodsState {
    /// Schema version for forward-compatible deserialization.
    pub version: String,
    /// The currently active pod, or `None` if no pod has been configured.
    #[serde(default)]
    pub active_pod: Option<PodState>,
}

impl Default for PodsState {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            active_pod: None,
        }
    }
}

/// Active pod definition persisted in `~/.vtcode/pods/state.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodState {
    /// Human-readable pod name.
    pub name: String,
    /// SSH connection string (e.g. `"ssh root@192.168.1.10"`).
    pub ssh: String,
    /// Optional path on the pod where model weights reside.
    #[serde(default)]
    pub models_path: Option<String>,
    /// GPU inventory for this pod.
    #[serde(default)]
    pub gpus: Vec<PodGpu>,
    /// Currently running models keyed by their user-facing name.
    #[serde(default)]
    pub models: BTreeMap<String, RunningModel>,
}

impl PodState {
    /// Return the total number of GPUs available on this pod.
    pub fn gpu_count(&self) -> usize {
        self.gpus.len()
    }
}

/// GPU inventory entry for a pod.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PodGpu {
    /// Device index as reported by `nvidia-smi`.
    pub id: u32,
    /// GPU product name (e.g. `"NVIDIA A100-SXM4-80GB"`).
    pub name: String,
}

/// Running model metadata stored for later inspection and shutdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningModel {
    /// Hugging Face model identifier.
    pub model: String,
    /// Port the vLLM server is listening on.
    pub port: u16,
    /// IDs of GPUs allocated to this model.
    #[serde(default)]
    pub gpu_ids: Vec<u32>,
    /// Remote process ID.
    pub pid: u32,
    /// Name of the catalog profile used to launch this model.
    pub profile: String,
}

/// Pod runtime health classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodHealth {
    /// Process is alive and the health endpoint responds.
    Running,
    /// Process is alive but the health endpoint is not yet ready.
    Starting,
    /// Process exited with a known failure pattern in the logs.
    Crashed,
    /// Process is not running.
    Dead,
}

impl PodHealth {
    /// Return the lowercase string representation of this health status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Starting => "starting",
            Self::Crashed => "crashed",
            Self::Dead => "dead",
        }
    }
}
