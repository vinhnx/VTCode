//! Bounded Desktop projection over the CLI's validated config service.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;
use toml::Value as TomlValue;
use vtcode_core::config::{
    ConfigReadRequest, ConfigReadResponse, ConfigService, ConfigWriteRequest, ConfigWriteStrategy, ConfigWriteTarget,
    loader::layers::ConfigLayerSource,
};

use super::protocol;

const MAX_SETTINGS: usize = 2_048;
const MAX_SETTING_BYTES: usize = 16 * 1024;
const MAX_CONFIG_PATH_BYTES: usize = 512;
const MAX_LAYER_VERSION_BYTES: usize = 256;
const MAX_SETTINGS_RESPONSE_BYTES: usize = 768 * 1024;

pub(super) async fn read(workspace: PathBuf) -> Result<protocol::ConfigSettingsSnapshot> {
    tokio::task::spawn_blocking(move || read_blocking(&workspace))
        .await
        .context("CLI config read worker stopped")?
}

pub(super) async fn write(
    workspace: PathBuf,
    params: protocol::ConfigSettingsWriteParams,
) -> Result<protocol::ConfigSettingsWriteResult> {
    tokio::task::spawn_blocking(move || write_blocking(&workspace, params))
        .await
        .context("CLI config write worker stopped")?
}

fn read_blocking(workspace: &Path) -> Result<protocol::ConfigSettingsSnapshot> {
    let response = ConfigService::read(ConfigReadRequest {
        workspace: workspace.to_path_buf(),
        runtime_overrides: vec![],
    })
    .context("Failed to read effective CLI configuration")?;
    Ok(project_snapshot(workspace, response))
}

fn project_snapshot(workspace: &Path, response: ConfigReadResponse) -> protocol::ConfigSettingsSnapshot {
    let catalog = project_settings(&response);
    let target_versions = [
        protocol::ConfigTarget::User,
        protocol::ConfigTarget::Workspace,
        protocol::ConfigTarget::Project,
    ]
    .into_iter()
    .map(|target| protocol::ConfigTargetVersion {
        target,
        version: target_version(workspace, &response, target),
    })
    .collect();

    protocol::ConfigSettingsSnapshot {
        settings: catalog.settings,
        target_versions,
        omitted_sensitive: catalog.omitted_sensitive,
        omitted_large: catalog.omitted_large,
        omitted_limit: catalog.omitted_limit,
    }
}

fn project_settings(response: &ConfigReadResponse) -> CatalogBuilder {
    let mut catalog = CatalogBuilder::default();
    if let Some(root) = response.effective_config.as_object() {
        for (key, value) in root {
            if is_sensitive_key(key) {
                catalog.omitted_sensitive += 1;
                continue;
            }
            let path = key.as_str();
            if !valid_path_segment(path) {
                catalog.omitted_large += 1;
                continue;
            }
            flatten_setting(&mut catalog, path, value, response);
        }
    }
    catalog.settings.sort_by(|left, right| left.path.cmp(&right.path));
    catalog
}

#[derive(Default)]
struct CatalogBuilder {
    settings: Vec<protocol::ConfigSetting>,
    encoded_bytes: usize,
    omitted_sensitive: usize,
    omitted_large: usize,
    omitted_limit: usize,
}

fn flatten_setting(catalog: &mut CatalogBuilder, path: &str, value: &Value, response: &ConfigReadResponse) {
    if catalog.settings.len() >= MAX_SETTINGS {
        catalog.omitted_limit += 1;
        return;
    }

    if let Some(object) = value.as_object()
        && !object.is_empty()
    {
        if object.keys().all(|key| valid_path_segment(key)) {
            for (key, child) in object {
                if is_sensitive_key(key) {
                    catalog.omitted_sensitive += 1;
                    continue;
                }
                let child_path = format!("{path}.{key}");
                if child_path.len() <= MAX_CONFIG_PATH_BYTES {
                    flatten_setting(catalog, &child_path, child, response);
                } else {
                    catalog.omitted_large += 1;
                }
            }
            return;
        }
    }

    if contains_sensitive_content(value) {
        catalog.omitted_sensitive += 1;
        return;
    }

    let Ok(value_bytes) = serde_json::to_vec(value) else {
        catalog.omitted_large += 1;
        return;
    };
    if value_bytes.len() > MAX_SETTING_BYTES || path.len() > MAX_CONFIG_PATH_BYTES {
        catalog.omitted_large += 1;
        return;
    }

    let entry_bytes = path
        .len()
        .saturating_mul(2)
        .saturating_add(value_bytes.len())
        .saturating_add(128);
    if catalog.encoded_bytes.saturating_add(entry_bytes) > MAX_SETTINGS_RESPONSE_BYTES {
        catalog.omitted_limit += 1;
        return;
    }

    catalog.settings.push(protocol::ConfigSetting {
        path: path.to_owned(),
        value: value.clone(),
        origin: setting_origin(response, path),
    });
    catalog.encoded_bytes += entry_bytes;
}

fn setting_origin(response: &ConfigReadResponse, path: &str) -> Option<String> {
    let origin = response.origins.get(path)?;
    let layer = response.layers.iter().find(|layer| layer.metadata.name == origin.name)?;
    Some(
        match &layer.source {
            ConfigLayerSource::System { .. } => "system",
            ConfigLayerSource::User { .. } => "user",
            ConfigLayerSource::Project { .. } => "project",
            ConfigLayerSource::Workspace { .. } => "workspace",
            ConfigLayerSource::Runtime => "runtime",
        }
        .to_owned(),
    )
}

fn target_version(workspace: &Path, response: &ConfigReadResponse, target: protocol::ConfigTarget) -> Option<String> {
    let core_target = core_target(target);
    let target_path = ConfigService::resolve_target_path(workspace, &core_target).ok()?;
    response
        .layers
        .iter()
        .find(|layer| {
            if layer.disabled_reason.is_some() || layer.error.is_some() {
                return false;
            }
            source_path(&layer.source).is_some_and(|path| same_path(path, &target_path))
        })
        .map(|layer| layer.metadata.version.clone())
}

fn source_path(source: &ConfigLayerSource) -> Option<&Path> {
    match source {
        ConfigLayerSource::User { file }
        | ConfigLayerSource::Project { file }
        | ConfigLayerSource::Workspace { file }
        | ConfigLayerSource::System { file } => Some(file),
        ConfigLayerSource::Runtime => None,
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = vtcode_commons::canonicalize(left).unwrap_or_else(|_| vtcode_commons::paths::normalize_path(left));
    let right = vtcode_commons::canonicalize(right).unwrap_or_else(|_| vtcode_commons::paths::normalize_path(right));
    left == right
}

fn core_target(target: protocol::ConfigTarget) -> ConfigWriteTarget {
    match target {
        protocol::ConfigTarget::User => ConfigWriteTarget::User,
        protocol::ConfigTarget::Workspace => ConfigWriteTarget::Workspace,
        protocol::ConfigTarget::Project => ConfigWriteTarget::Project,
    }
}

fn write_blocking(
    workspace: &Path,
    params: protocol::ConfigSettingsWriteParams,
) -> Result<protocol::ConfigSettingsWriteResult> {
    validate_path(&params.path)?;
    if params
        .expected_layer_version
        .as_deref()
        .is_some_and(|version| version.is_empty() || version.len() > MAX_LAYER_VERSION_BYTES)
    {
        bail!("Invalid CLI config layer version");
    }
    if is_sensitive_path(&params.path) || contains_sensitive_content(&params.value) {
        bail!("Credential-related settings must be changed through VT Code authentication settings");
    }
    if serde_json::to_vec(&params.value)?.len() > MAX_SETTING_BYTES {
        bail!("CLI setting value exceeds the Desktop editor size limit");
    }

    let snapshot = read_blocking(workspace)?;
    let current = snapshot
        .settings
        .iter()
        .find(|setting| setting.path == params.path)
        .with_context(|| format!("Setting '{}' is not available in the editable CLI settings catalog", params.path))?;
    if !same_json_kind(&current.value, &params.value) {
        bail!("Setting '{}' must keep its existing value type", params.path);
    }

    let value = TomlValue::try_from(params.value).context("Setting value is not representable in TOML")?;
    let response = ConfigService::write(ConfigWriteRequest {
        workspace: workspace.to_path_buf(),
        target: core_target(params.target),
        path: params.path.clone(),
        value,
        strategy: ConfigWriteStrategy::Replace,
        expected_layer_version: params.expected_layer_version,
    })
    .context("Failed to validate and save CLI configuration")?;

    Ok(protocol::ConfigSettingsWriteResult {
        target: params.target,
        path: params.path,
        written_layer_version: response.written_layer_version,
        overridden: response.overridden_metadata.is_some(),
    })
}

fn validate_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > MAX_CONFIG_PATH_BYTES
        || path
            .split('.')
            .any(|segment| segment.is_empty() || !valid_path_segment(segment))
    {
        bail!("Invalid CLI config setting path");
    }
    if is_sensitive_path(path) {
        bail!("Credential-related settings must be changed through VT Code authentication settings");
    }
    Ok(())
}

fn valid_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment
            .chars()
            .any(|character| character.is_control() || matches!(character, '.' | '[' | ']'))
        && segment.len() <= MAX_CONFIG_PATH_BYTES
}

fn is_sensitive_path(path: &str) -> bool {
    path.split('.').any(is_sensitive_key)
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    if normalized == "api_key_env" {
        return false;
    }
    matches!(
        normalized.as_str(),
        "api_key"
            | "access_token"
            | "refresh_token"
            | "token"
            | "password"
            | "secret"
            | "client_secret"
            | "authorization"
            | "headers"
            | "http_headers"
            | "credentials"
            | "private_key"
    ) || normalized.ends_with("_secret")
        || normalized.ends_with("_token")
        || normalized.ends_with("_password")
        || normalized.ends_with("_api_key")
}

fn contains_sensitive_content(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(contains_sensitive_content),
        Value::Object(object) => object
            .iter()
            .any(|(key, value)| is_sensitive_key(key) || contains_sensitive_content(value)),
        Value::String(value) => contains_url_credentials(value),
        _ => false,
    }
}

fn contains_url_credentials(value: &str) -> bool {
    url::Url::parse(value).ok().is_some_and(|url| {
        !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query_pairs().any(|(key, _)| is_sensitive_url_query_key(&key))
    })
}

fn is_sensitive_url_query_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    matches!(
        normalized.as_str(),
        "key"
            | "api_key"
            | "access_token"
            | "refresh_token"
            | "token"
            | "password"
            | "secret"
            | "client_secret"
            | "authorization"
    )
}

fn same_json_kind(current: &Value, replacement: &Value) -> bool {
    match current {
        Value::Null => false,
        Value::Bool(_) => replacement.is_boolean(),
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            replacement.as_number().is_some_and(|number| number.is_i64() || number.is_u64())
        }
        Value::Number(_) => replacement.as_number().is_some_and(serde_json::Number::is_f64),
        Value::String(_) => replacement.is_string(),
        Value::Array(_) => replacement.is_array(),
        Value::Object(_) => replacement.is_object(),
    }
}

#[cfg(test)]
mod tests {
    use super::{contains_sensitive_content, is_sensitive_key, project_settings, same_json_kind, validate_path};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use vtcode_core::config::{ConfigLayerView, ConfigReadResponse};

    fn empty_response() -> ConfigReadResponse {
        ConfigReadResponse {
            effective_config: json!({}),
            merged_version: "version".into(),
            layers: Vec::<ConfigLayerView>::new(),
            origins: BTreeMap::new(),
        }
    }

    #[test]
    fn settings_projection_hides_credential_leaves_but_keeps_safe_siblings() {
        let response = empty_response();
        let response = ConfigReadResponse {
            effective_config: json!({
            "agent": {
                "default_model": "model-a",
                "api_key": "must-not-escape"
            },
            "mcp": {
                "providers": {
                    "local": {"http_headers": {"Authorization": "must-not-escape"}}
                }
            },
            "custom_provider": {"api_key_env": "MY_PROVIDER_KEY"}
            }),
            ..response
        };
        let settings = project_settings(&response);
        assert_eq!(settings.settings.len(), 2);
        assert_eq!(settings.settings[0].path, "agent.default_model");
        assert_eq!(settings.settings[0].value, json!("model-a"));
        assert_eq!(settings.settings[1].path, "custom_provider.api_key_env");
        assert_eq!(settings.settings[1].value, json!("MY_PROVIDER_KEY"));
        assert_eq!(settings.omitted_sensitive, 2);
        assert!(
            !serde_json::to_string(&settings.settings)
                .expect("settings JSON")
                .contains("must-not-escape")
        );
    }

    #[test]
    fn settings_projection_omits_urls_with_inline_credentials() {
        let response = ConfigReadResponse {
            effective_config: json!({
                "provider_overrides": {
                    "safe": {"base_url": "https://api.example/v1"},
                    "credentialed": {"base_url": "https://user:password@example.test/v1"},
                    "query_token": {"base_url": "https://api.example/v1?access_token=hidden"}
                }
            }),
            ..empty_response()
        };
        let settings = project_settings(&response);
        assert_eq!(settings.settings.len(), 1);
        assert_eq!(settings.settings[0].path, "provider_overrides.safe.base_url");
        assert_eq!(settings.omitted_sensitive, 2);
        assert!(contains_sensitive_content(&json!("https://user:password@example.test/v1")));
        assert!(contains_sensitive_content(&json!("https://api.example/v1?access_token=hidden")));
    }

    #[test]
    fn config_setting_paths_reject_ambiguous_segments() {
        assert!(validate_path("agent.model").is_ok());
        assert!(validate_path("agent..model").is_err());
        assert!(validate_path("agent.model[0]").is_err());
        assert!(validate_path("provider.api-key").is_err());
        assert!(validate_path("provider.api_key").is_err());
        assert!(validate_path(&"x".repeat(513)).is_err());
    }

    #[test]
    fn settings_projection_bounds_catalog_and_value_sizes() {
        let mut config = serde_json::Map::new();
        config.insert("large_value".into(), json!("x".repeat(16 * 1024 + 1)));
        for index in 0..2_050 {
            config.insert(format!("setting_{index:04}"), json!(index));
        }
        let response = ConfigReadResponse {
            effective_config: Value::Object(config),
            ..empty_response()
        };
        let settings = project_settings(&response);
        assert_eq!(settings.settings.len(), 2_048);
        assert_eq!(settings.omitted_large, 1);
        assert_eq!(settings.omitted_limit, 2);
    }

    #[test]
    fn settings_projection_stays_well_below_the_protocol_frame_limit() {
        let mut config = serde_json::Map::new();
        for index in 0..128 {
            config.insert(format!("large_{index:03}"), json!("x".repeat(8 * 1024)));
        }
        let response = ConfigReadResponse {
            effective_config: Value::Object(config),
            ..empty_response()
        };
        let settings = project_settings(&response);
        let encoded = serde_json::to_vec(&settings.settings).expect("settings JSON");
        assert!(!settings.settings.is_empty());
        assert!(encoded.len() <= super::MAX_SETTINGS_RESPONSE_BYTES);
        assert!(settings.omitted_limit > 0);
    }

    #[test]
    fn config_writes_reject_credentials_and_oversized_values_before_io() {
        let workspace = std::path::Path::new("/workspace-does-not-need-to-exist");
        let credential_url = super::protocol::ConfigSettingsWriteParams {
            target: super::protocol::ConfigTarget::Workspace,
            path: "provider_overrides.local.base_url".into(),
            value: json!("https://user:password@example.test/v1"),
            expected_layer_version: None,
        };
        assert!(super::write_blocking(workspace, credential_url).is_err());

        let oversized = super::protocol::ConfigSettingsWriteParams {
            target: super::protocol::ConfigTarget::Workspace,
            path: "agent.default_model".into(),
            value: json!("x".repeat(16 * 1024 + 1)),
            expected_layer_version: None,
        };
        assert!(super::write_blocking(workspace, oversized).is_err());

        let invalid_version = super::protocol::ConfigSettingsWriteParams {
            target: super::protocol::ConfigTarget::Workspace,
            path: "agent.default_model".into(),
            value: json!("model"),
            expected_layer_version: Some("x".repeat(257)),
        };
        assert!(super::write_blocking(workspace, invalid_version).is_err());
    }

    #[test]
    fn config_editor_requires_values_to_keep_the_original_toml_shape() {
        assert!(is_sensitive_key("api-key"));
        assert!(!is_sensitive_key("api_key_env"));
        assert!(same_json_kind(&json!(true), &json!(false)));
        assert!(!same_json_kind(&json!(true), &json!("false")));
        assert!(same_json_kind(&json!(3), &json!(4)));
        assert!(!same_json_kind(&json!(3), &json!(4.5)));
        assert!(same_json_kind(&json!("old"), &json!("new")));
        assert!(same_json_kind(&json!(["a"]), &json!(["b"])));
        assert!(same_json_kind(&json!({"a": 1}), &json!({"b": 2})));
    }
}
