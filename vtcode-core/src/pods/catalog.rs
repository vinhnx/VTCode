use crate::pods::state::PodState;
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
    pub name: String,
    pub model: String,
    pub gpu_count: usize,
    #[serde(default)]
    pub gpu_types: Vec<String>,
    #[serde(default = "default_command_template")]
    pub command_template: String,
    #[serde(default)]
    pub vllm_args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl PodCatalog {
    pub fn embedded_default() -> Self {
        match serde_json::from_str(include_str!("default_catalog.json")) {
            Ok(catalog) => catalog,
            Err(_) => Self {
                version: "1".to_string(),
                profiles: vec![PodProfile {
                    name: "generic-8b".to_string(),
                    model: "meta-llama/Llama-3.1-8B-Instruct".to_string(),
                    gpu_count: 1,
                    gpu_types: vec![],
                    command_template: default_command_template(),
                    vllm_args: vec![
                        "--trust-remote-code".to_string(),
                        "--dtype".to_string(),
                        "auto".to_string(),
                        "--gpu-memory-utilization".to_string(),
                        "0.90".to_string(),
                        "--max-model-len".to_string(),
                        "8192".to_string(),
                    ],
                    env: BTreeMap::new(),
                }],
            },
        }
    }

    pub fn profiles_for_model(&self, model: &str) -> Vec<&PodProfile> {
        self.profiles
            .iter()
            .filter(|profile| profile.name == model || profile.model == model)
            .collect()
    }

    pub fn compatible_profiles<'a>(
        &'a self,
        pod: &PodState,
    ) -> (Vec<&'a PodProfile>, Vec<&'a PodProfile>) {
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
            gpus: vec![PodGpu {
                id: 0,
                name: "NVIDIA A100-SXM4-80GB".to_string(),
            }],
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
                PodGpu {
                    id: 0,
                    name: "NVIDIA A100-SXM4-80GB".to_string(),
                },
                PodGpu {
                    id: 1,
                    name: "NVIDIA RTX 4090".to_string(),
                },
            ],
            models: BTreeMap::new(),
        };

        assert!(!profile.matches_pod(&pod));
    }
}
