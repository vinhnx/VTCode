use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Root state for VT Code pod management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodsState {
    pub version: String,
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
    pub name: String,
    pub ssh: String,
    #[serde(default)]
    pub models_path: Option<String>,
    #[serde(default)]
    pub gpus: Vec<PodGpu>,
    #[serde(default)]
    pub models: BTreeMap<String, RunningModel>,
}

impl PodState {
    pub fn gpu_count(&self) -> usize {
        self.gpus.len()
    }
}

/// GPU inventory entry for a pod.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PodGpu {
    pub id: u32,
    pub name: String,
}

/// Running model metadata stored for later inspection and shutdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningModel {
    pub model: String,
    pub port: u16,
    #[serde(default)]
    pub gpu_ids: Vec<u32>,
    pub pid: u32,
    pub profile: String,
}

/// Pod runtime health classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodHealth {
    Running,
    Starting,
    Crashed,
    Dead,
}

impl PodHealth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Starting => "starting",
            Self::Crashed => "crashed",
            Self::Dead => "dead",
        }
    }
}
