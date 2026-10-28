use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use tokio::task;
use vtcode_core::SimpleIndexer;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

use crate::agent::agents::apply_live_reload_overrides;

async fn blocking_task<F, R>(label: &str, f: F) -> Result<R>
where
    F: FnOnce() -> Result<R> + Send + 'static,
    R: Send + 'static,
{
    task::spawn_blocking(f)
        .await
        .map_err(|err| anyhow!("failed to join {label} task: {err}"))?
}

pub(crate) async fn bootstrap_config_files(workspace: PathBuf, force: bool) -> Result<Vec<String>> {
    blocking_task("configuration bootstrap", move || VTCodeConfig::bootstrap_project(&workspace, force)).await
}

pub(crate) async fn build_workspace_index(workspace: PathBuf) -> Result<()> {
    blocking_task("workspace indexing", move || -> Result<()> {
        let mut indexer = SimpleIndexer::new(workspace.clone());
        indexer.init()?;
        indexer.index_directory(&workspace)?;
        Ok(())
    })
    .await
}

async fn load_workspace_config_snapshot(workspace: &Path) -> Result<VTCodeConfig> {
    let workspace_buf = workspace.to_path_buf();
    blocking_task("workspace config load", move || {
        ConfigManager::invalidate_workspace_cache(&workspace_buf);
        ConfigManager::load_from_workspace_with_repository_repair(&workspace_buf)
            .map(|manager| manager.config().clone())
    })
    .await
}

pub(crate) async fn refresh_vt_config(
    workspace: &Path,
    runtime_cfg: &CoreAgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
) -> Result<()> {
    let snapshot = load_workspace_config_snapshot(workspace).await?;
    apply_workspace_config_snapshot(snapshot, runtime_cfg, vt_cfg)
}

pub(crate) fn apply_workspace_config_snapshot(
    mut snapshot: VTCodeConfig,
    runtime_cfg: &CoreAgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
) -> Result<()> {
    apply_live_reload_overrides(Some(&mut snapshot), runtime_cfg);
    // Configuration reloads must not silently change the session's explicit
    // execution mode, which is intentionally absent from persisted config.
    if vt_cfg.as_ref().is_some_and(|old| old.sandbox.is_full_access()) {
        snapshot.sandbox.enable_full_access();
    }
    // Skip the remove+register cycle when custom providers haven't changed.
    // This function is called on every turn and on live-reload; re-registering
    // unchanged providers wastes factory lock + closure rebuild work and
    // produces repeated "Registered custom provider" log noise.
    let custom_providers_unchanged = vt_cfg
        .as_ref()
        .is_some_and(|old| old.custom_providers == snapshot.custom_providers);
    if !custom_providers_unchanged {
        vtcode_core::llm::factory::register_custom_providers(&snapshot.custom_providers);
    }
    *vt_cfg = Some(snapshot);
    Ok(())
}

pub(crate) fn apply_workspace_config_to_registry(
    tool_registry: &vtcode_core::tools::registry::ToolRegistry,
    vt_cfg: &VTCodeConfig,
) -> Result<()> {
    tool_registry.apply_commands_config(&vt_cfg.commands);
    tool_registry.apply_permissions_config(&vt_cfg.permissions);
    tool_registry.apply_sandbox_config(&vt_cfg.sandbox);
    tool_registry.apply_timeout_policy(&vt_cfg.timeouts);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_config() -> CoreAgentConfig {
        CoreAgentConfig {
            model: "test-model".to_string(),
            api_key: String::new(),
            provider: "anthropic".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            workspace: std::env::temp_dir(),
            verbose: false,
            quiet: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: vtcode_config::types::ReasoningEffortLevel::default(),
            ui_surface: vtcode_core::config::types::UiSurfacePreference::default(),
            prompt_cache: vtcode_core::config::core::PromptCachingConfig::default(),
            model_source: vtcode_core::config::types::ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: std::collections::BTreeMap::new(),
            checkpointing_enabled: false,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: 1,
            checkpointing_max_age_days: None,
            max_conversation_turns: 1000,
            model_behavior: None,
            openai_chatgpt_auth: None,
        }
    }

    #[test]
    fn config_snapshot_preserves_explicit_full_access() {
        let runtime = runtime_config();
        let mut old = VTCodeConfig::default();
        old.sandbox.enable_full_access();
        let mut vt_cfg = Some(old);

        apply_workspace_config_snapshot(VTCodeConfig::default(), &runtime, &mut vt_cfg).expect("snapshot applies");
        let cfg = vt_cfg.as_ref().expect("config present");
        assert!(cfg.sandbox.is_full_access());
        // Preservation must not rewrite the configured sandbox policy.
        assert_eq!(cfg.sandbox.default_policy, vtcode_config::SandboxPolicy::ReadOnly);
        assert!(!cfg.sandbox.enabled);
    }

    #[test]
    fn config_snapshot_without_prior_mode_stays_configured() {
        let runtime = runtime_config();
        let mut vt_cfg = Some(VTCodeConfig::default());

        apply_workspace_config_snapshot(VTCodeConfig::default(), &runtime, &mut vt_cfg).expect("snapshot applies");
        assert!(!vt_cfg.as_ref().expect("config present").sandbox.is_full_access());
    }
}
