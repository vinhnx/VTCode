use super::state::PodState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Root catalog describing known deployment profiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodCatalog {
    pub version: String,
    #[serde(default)]
    pub profiles: Vec<PodProfile>,
}

impl Default for PodCatalog {
    fn default() -> Self {
        Self::embedded_default()
    }
}

/// A single deployment profile for a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodProfile {
    /// Short identifier for this profile.
    pub name: String,
    /// Hugging Face model identifier.
    pub model: String,
    /// Number of GPUs required by this profile.
    pub gpu_count: usize,
    /// Optional GPU type constraints (substring-matched against GPU names).
    #[serde(default)]
    pub gpu_types: Vec<String>,
    /// Command template with `{{MODEL_ID}}`, `{{NAME}}`, `{{PORT}}`, and `{{VLLM_ARGS}}` placeholders.
    #[serde(default = "default_command_template")]
    pub command_template: String,
    /// Extra command-line arguments passed to vLLM.
    #[serde(default)]
    pub vllm_args: Vec<String>,
    /// Environment variables set before launching the server.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl PodCatalog {
    /// Return the built-in catalog compiled into the binary.
    pub fn embedded_default() -> Self {
        match serde_json::from_str(include_str!("default_catalog.json")) {
            Ok(catalog) => catalog,
            Err(_) => Self {
                version: "2".to_string(),
                profiles: vec![PodProfile {
                    name: "qwen3-30b-a3b".to_string(),
                    model: "Qwen/Qwen3-30B-A3B".to_string(),
                    gpu_count: 1,
                    gpu_types: vec![],
                    command_template: default_command_template(),
                    vllm_args: vec![
                        "--trust-remote-code".to_string(),
                        "--dtype".to_string(),
                        "bfloat16".to_string(),
                        "--gpu-memory-utilization".to_string(),
                        "0.90".to_string(),
                        "--max-model-len".to_string(),
                        "32768".to_string(),
                    ],
                    env: BTreeMap::new(),
                }],
            },
        }
    }

    /// Return all profiles whose name or model field matches `model`.
    pub fn profiles_for_model(&self, model: &str) -> Vec<&PodProfile> {
        self.profiles
            .iter()
            .filter(|profile| profile.name == model || profile.model == model)
            .collect()
    }

    /// Split all profiles into those compatible with `pod` and those that are not.
    pub fn compatible_profiles<'a>(&'a self, pod: &PodState) -> (Vec<&'a PodProfile>, Vec<&'a PodProfile>) {
        let mut compatible = Vec::new();
        let mut incompatible = Vec::new();

        for profile in &self.profiles {
            if profile.matches_pod(pod) {
                compatible.push(profile);
            } else {
                incompatible.push(profile);
            }
        }

        (compatible, incompatible)
    }
}

impl PodProfile {
    /// Return `true` if `pod` has enough GPUs of the required type for this profile.
    pub fn matches_pod(&self, pod: &PodState) -> bool {
        if self.gpu_count > pod.gpu_count() {
            return false;
        }

        if self.gpu_types.is_empty() {
            return true;
        }

        let gpu_types = self
            .gpu_types
            .iter()
            .map(|gpu_type| gpu_type.to_lowercase())
            .collect::<Vec<_>>();

        pod.gpus
            .iter()
            .filter(|gpu| {
                let gpu_name = gpu.name.to_lowercase();
                gpu_types.iter().any(|gpu_type| gpu_name.contains(gpu_type))
            })
            .take(self.gpu_count)
            .count()
            >= self.gpu_count
    }

    /// Return `true` if the profile requires exactly `count` GPUs.
    pub fn matches_gpu_count(&self, count: usize) -> bool {
        self.gpu_count == count
    }
}

fn default_command_template() -> String {
    "vllm serve {{MODEL_ID}} --served-model-name {{NAME}} --port {{PORT}} {{VLLM_ARGS}}".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pods::state::{PodGpu, PodState};

    #[test]
    fn profile_matches_gpu_types_by_substring() {
        let profile = PodProfile {
            name: "test".to_string(),
            model: "model".to_string(),
            gpu_count: 1,
            gpu_types: vec!["A100".to_string()],
            command_template: default_command_template(),
            vllm_args: vec![],
            env: BTreeMap::new(),
        };
        let pod = PodState {
            name: "pod".to_string(),
            ssh: "ssh root@example.com".to_string(),
            models_path: None,
            gpus: vec![PodGpu { id: 0, name: "NVIDIA A100-SXM4-80GB".to_string() }],
            models: BTreeMap::new(),
        };

        assert!(profile.matches_pod(&pod));
    }

    #[test]
    fn profile_requires_enough_matching_gpu_types() {
        let profile = PodProfile {
            name: "dual-a100".to_string(),
            model: "model".to_string(),
            gpu_count: 2,
            gpu_types: vec!["A100".to_string()],
            command_template: default_command_template(),
            vllm_args: vec![],
            env: BTreeMap::new(),
        };
        let pod = PodState {
            name: "pod".to_string(),
            ssh: "ssh root@example.com".to_string(),
            models_path: None,
            gpus: vec![
                PodGpu { id: 0, name: "NVIDIA A100-SXM4-80GB".to_string() },
                PodGpu { id: 1, name: "NVIDIA RTX 4090".to_string() },
            ],
            models: BTreeMap::new(),
        };

        assert!(!profile.matches_pod(&pod));
    }
}
