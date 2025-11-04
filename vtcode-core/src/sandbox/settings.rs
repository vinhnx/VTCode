use std::path::Path;

use serde::{Deserialize, Serialize};

use super::SandboxRuntimeKind;

/// Serializable representation of sandbox configuration persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxSettings {
    /// Runtime metadata section consumed by the sandbox CLI.
    pub sandbox: SandboxRuntimeConfig,
    /// Permission rules that gate filesystem and network access.
    pub permissions: SandboxPermissions,
}

impl SandboxSettings {
    /// Construct a new settings payload using the provided runtime metadata and permissions.
    pub fn new(
        runtime_kind: SandboxRuntimeKind,
        settings_path: impl AsRef<Path>,
        persistent_storage: impl AsRef<Path>,
        allow_rules: Vec<String>,
        deny_rules: Vec<String>,
        allowed_paths: Vec<String>,
        allowed_domains: Vec<String>,
    ) -> Self {
        Self {
            sandbox: SandboxRuntimeConfig {
                enabled: true,
                runtime: runtime_kind.as_str().to_string(),
                settings_path: settings_path.as_ref().display().to_string(),
                persistent_storage: persistent_storage.as_ref().display().to_string(),
            },
            permissions: SandboxPermissions {
                allow: allow_rules,
                deny: deny_rules,
                allowed_paths,
                network: SandboxNetworkPermissions { allowed_domains },
            },
        }
    }

    /// Convert the settings to a `serde_json::Value`.
    pub fn to_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }

    /// Render the settings as pretty printed JSON suitable for writing to disk.
    pub fn to_pretty_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

/// Runtime configuration information stored under the `sandbox` section of `settings.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxRuntimeConfig {
    pub enabled: bool,
    pub runtime: String,
    pub settings_path: String,
    pub persistent_storage: String,
}

/// Permission configuration stored alongside sandbox metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxPermissions {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    pub network: SandboxNetworkPermissions,
}

/// Network-specific permission settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxNetworkPermissions {
    #[serde(default)]
    pub allowed_domains: Vec<String>,
}
