//! Configuration snapshots, settings catalog, and model picker DTOs.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::secret::SecretStoreInfo;
use super::session::SessionOptionsSnapshot;

/// Basic workspace and provider configuration exposed to the GUI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationSnapshot {
    /// Active provider identifier.
    pub provider: String,
    /// Active model identifier.
    pub model: String,
    /// Whether provider authentication requirements passed startup preflight.
    pub authenticated: bool,
    /// Existing CLI command users can run to configure credentials.
    pub credential_setup_command: String,
    /// Human label for the execution policy the service runs under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    /// Context window of the active model, when the catalog knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Non-secret CLI UI preferences available to the native client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_ui: Option<SharedUiPreferences>,
    /// In-app secret-form support for the active provider, absent for
    /// login-flow/local providers and older services.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_store: Option<SecretStoreInfo>,
    /// Effective options for the next runtime session, absent on older helpers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_options: Option<SessionOptionsSnapshot>,
}

/// Writable configuration layer understood by the CLI config service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigTarget {
    /// User-level configuration.
    User,
    /// Workspace-level configuration.
    Workspace,
    /// Project profile configuration.
    Project,
}

/// Non-secret setting value exposed to the native configuration browser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSetting {
    /// Stable dotted path accepted by the CLI config writer.
    pub path: String,
    /// Effective CLI value after defaults; null represents an unset documented optional field.
    pub value: Value,
    /// Safe layer label such as `workspace` or `user`; never a filesystem path.
    pub origin: Option<String>,
    /// CLI `/config` display label, when documented.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// CLI schema type used when a setting has no explicit effective value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    /// Documented default value, rendered as plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    /// Description from the shared CLI configuration reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Documented choices, if the CLI exposes a finite set.
    #[serde(default)]
    pub options: Vec<String>,
    /// Curated CLI `/config` group or `Advanced settings`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Purpose of the curated group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_description: Option<String>,
    /// Whether the current CLI catalog considers this a writable setting.
    #[serde(default = "config_setting_editable_default")]
    pub editable: bool,
}

const fn config_setting_editable_default() -> bool {
    true
}

/// Version of one writable config layer, for optimistic write protection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigTargetVersion {
    /// Layer represented by this version.
    pub target: ConfigTarget,
    /// Layer fingerprint, when the selected layer was loaded.
    pub version: Option<String>,
}

/// Bounded, credential-filtered catalog of the CLI's effective and documented settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSettingsSnapshot {
    /// Effective settings and documented schema entries from the shared CLI `/config` catalog.
    pub settings: Vec<ConfigSetting>,
    /// Fingerprints for user, workspace, and project layers.
    pub target_versions: Vec<ConfigTargetVersion>,
    /// Number of credential-bearing branches excluded from the catalog.
    pub omitted_sensitive: usize,
    /// Number of values omitted because they exceeded the UI size bound.
    pub omitted_large: usize,
    /// Number of additional settings omitted because the catalog reached its limit.
    pub omitted_limit: usize,
}

/// Write one setting through the shared, validated CLI configuration service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigSettingsWriteParams {
    /// Layer to update.
    pub target: ConfigTarget,
    /// Dotted setting path from `ConfigSettingsSnapshot`.
    pub path: String,
    /// Replacement value. Its type must match the effective setting or documented schema type.
    pub value: Value,
    /// Version observed when the setting editor opened.
    pub expected_layer_version: Option<String>,
}

/// Result of persisting a CLI configuration value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSettingsWriteResult {
    /// Layer updated by the CLI config service.
    pub target: ConfigTarget,
    /// Setting path that was persisted.
    pub path: String,
    /// Version of the updated layer.
    pub written_layer_version: String,
    /// Whether a higher-precedence layer still controls the effective value.
    pub overridden: bool,
}

/// Open one layered config file in the platform default editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigOpenFileParams {
    /// Layer whose file should be opened.
    pub target: ConfigTarget,
}

fn default_communication_style() -> String {
    "balanced".to_string()
}

/// CLI preferences projected through existing parsers rather than a second config loader.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedUiPreferences {
    /// Theme ID resolved by the CLI startup loader.
    pub theme: String,
    /// Effective `ui.communication_style` (`balanced`/`quiet`/`verbose`).
    /// Absent on older services; the desktop falls back to `balanced`.
    #[serde(default = "default_communication_style")]
    pub communication_style: String,
    /// Desktop command ID to normalized native key combinations; an empty list unbinds it.
    #[serde(default)]
    pub shortcuts: std::collections::BTreeMap<String, Vec<String>>,
    /// Explicit diagnostics for skipped bindings; contains no configuration secrets.
    #[serde(default)]
    pub shortcut_warnings: Vec<String>,
}

/// One selectable model from the service's catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// Provider identifier the model runs on.
    pub provider: String,
    /// Canonical model identifier.
    pub id: String,
    /// Human-friendly display name.
    pub display_name: String,
    /// Context window in tokens, when the catalog knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Whether this is the currently active provider/model.
    #[serde(default)]
    pub current: bool,
    /// Position in the CLI's recommended model set, if the model is recommended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_rank: Option<u8>,
    /// Capability details sourced from the CLI's model catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ModelCapabilities>,
}

/// Non-secret model capabilities used to explain model choices in the desktop picker.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    /// Supports reasoning controls.
    #[serde(default)]
    pub reasoning: bool,
    /// Accepts image input.
    #[serde(default)]
    pub vision: bool,
    /// Supports tool calls.
    #[serde(default)]
    pub tool_call: bool,
    /// Supports structured output.
    #[serde(default)]
    pub structured_output: bool,
    /// Identifies a pro-tier model.
    #[serde(default)]
    pub is_pro: bool,
    /// Input modalities advertised by the CLI catalog.
    #[serde(default)]
    pub input_modalities: Vec<String>,
}

/// Parameters for selecting a provider and model. The service restarts the
/// selected session's runtime under the same session id; the thread keeps its
/// history and the new route applies to subsequent turns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelectParams {
    /// Provider identifier.
    pub provider: String,
    /// Canonical model identifier.
    pub model: String,
}
