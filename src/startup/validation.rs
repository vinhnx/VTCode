use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::validator::ConfigValidator;
use vtcode_core::utils::path::canonicalize_workspace;

pub(super) fn apply_cli_permission_overrides(
    config: &mut VTCodeConfig,
    allowed_tools: &[String],
    disallowed_tools: &[String],
) {
    for entry in iter_permission_entries(allowed_tools) {
        push_unique_permission_entry(&mut config.permissions.allow, entry);
    }

    for entry in iter_permission_entries(disallowed_tools) {
        push_unique_permission_entry(&mut config.permissions.deny, entry);
    }
}

fn iter_permission_entries(entries: &[String]) -> impl Iterator<Item = &str> {
    entries
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn push_unique_permission_entry(target: &mut Vec<String>, entry: &str) {
    if !target.iter().any(|existing| existing == entry) {
        target.push(entry.to_string());
    }
}

pub(super) fn parse_cli_config_entries(
    entries: &[String],
) -> (Option<PathBuf>, Vec<(String, String)>) {
    let mut config_path: Option<PathBuf> = None;
    let mut overrides = Vec::new();

    for entry in entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            overrides.push((key.to_owned(), value.trim().to_owned()));
        } else if config_path.is_none() {
            config_path = Some(PathBuf::from(trimmed));
        }
    }

    (config_path, overrides)
}

pub(super) fn resolve_config_path(workspace: &Path, candidate: &Path) -> PathBuf {
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }

    let workspace_candidate = workspace.join(candidate);
    if workspace_candidate.exists() {
        return workspace_candidate;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| workspace.to_path_buf());
    let cwd_candidate = cwd.join(candidate);
    if cwd_candidate.exists() {
        cwd_candidate
    } else {
        workspace_candidate
    }
}

pub(super) fn validate_full_auto_configuration(
    config: &VTCodeConfig,
    workspace: &Path,
) -> Result<()> {
    let automation_cfg = &config.automation.full_auto;
    if !automation_cfg.enabled {
        bail!(
            "Full-auto permission review is disabled in configuration. Enable it under [automation.full_auto]."
        );
    }

    if automation_cfg.require_profile_ack {
        let profile_path = automation_cfg.profile_path.clone().ok_or_else(|| {
            anyhow!(
                "Full-auto permission review requires 'profile_path' in [automation.full_auto] when require_profile_ack = true."
            )
        })?;
        let resolved_profile = if profile_path.is_absolute() {
            profile_path
        } else {
            workspace.join(profile_path)
        };

        if !resolved_profile.exists() {
            bail!(
                "Full-auto profile '{}' not found. Create the acknowledgement file before using --full-auto.",
                resolved_profile.display()
            );
        }
    }

    Ok(())
}

pub(super) fn resolve_workspace_path(workspace_arg: Option<PathBuf>) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Failed to determine current working directory")?;

    let resolved = match workspace_arg {
        Some(path) if path.is_absolute() => path,
        Some(path) => cwd.join(path),
        None => cwd,
    };

    Ok(canonicalize_workspace(&resolved))
}

pub(super) fn validate_startup_configuration(
    config: &VTCodeConfig,
    _workspace: &Path,
    quiet: bool,
) -> Result<()> {
    // Ripgrep availability is checked lazily when search tools are actually
    // needed (session setup, first-run, `vtcode dependencies`).  Checking it
    // here would fork a subprocess (`rg --version`) on every startup for
    // purely informational logging — not worth the 50-200ms cost.

    let validator = ConfigValidator::generated();
    if let Err(e) = validator.validate(config)
        && !quiet
    {
        tracing::warn!("could not validate configured model catalog: {e}");
    }

    if !quiet {
        warn_token_overhead(config);
    }

    Ok(())
}

/// Returns non-fatal warnings describing configuration choices that are likely
/// to inflate the per-request token cost:
///
/// - many configured MCP servers (schema tax on every request),
/// - heavy `system_prompt_mode` / `tool_documentation_mode`,
/// - disabled tool-result clearing (unbounded context growth).
///
/// Kept pure (no logging side effects) so it can be unit-tested in isolation;
/// `warn_token_overhead` is the logging wrapper.
fn collect_token_overhead_warnings(config: &VTCodeConfig) -> Vec<String> {
    use vtcode_core::config::{SystemPromptMode, ToolDocumentationMode};

    const MCP_SERVER_OVERHEAD_WARN_THRESHOLD: usize = 8;

    let mut warnings = Vec::new();

    let mcp_servers = config.mcp.providers.len();
    if mcp_servers > MCP_SERVER_OVERHEAD_WARN_THRESHOLD {
        warnings.push(format!(
            "configured {mcp_servers} MCP servers (threshold {MCP_SERVER_OVERHEAD_WARN_THRESHOLD}); each server's tool schemas are sent on every request unless deferred. Consider reducing the count or relying on deferred tool loading (tools.client_tool_search defaults to true) to lower token cost."
        ));
    }

    if matches!(config.agent.system_prompt_mode, SystemPromptMode::Specialized) {
        warnings.push(
            "agent.system_prompt_mode = 'specialized' sends a larger base system prompt on every request. Prefer 'minimal' or 'lightweight' (default) to reduce token cost.".to_string(),
        );
    }

    if matches!(
        config.agent.tool_documentation_mode,
        ToolDocumentationMode::Full
    ) {
        warnings.push(
            "agent.tool_documentation_mode = 'full' sends complete tool documentation on every request. Prefer 'progressive' (default) to keep tool schemas small.".to_string(),
        );
    }

    if !config.agent.harness.tool_result_clearing.enabled {
        warnings.push(
            "agent.harness.tool_result_clearing is disabled; old tool results accumulate in context and raise per-turn token cost. Enable it (the default) to bound context growth.".to_string(),
        );
    }

    warnings
}

/// Logs the warnings returned by `collect_token_overhead_warnings`. Token
/// efficiency is a correctness concern — every harness token is context the
/// model cannot spend on the task — so surfacing these at startup lets users
/// audit and trim their setup before paying for it.
fn warn_token_overhead(config: &VTCodeConfig) {
    for warning in collect_token_overhead_warnings(config) {
        tracing::warn!("{warning}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;
    use std::env;
    use vtcode_config::McpProviderConfig;
    use vtcode_commons::env_lock;
    use vtcode_core::config::loader::ConfigBuilder;

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "test function, assertions are OK"
    )]
    fn resolves_current_dir_when_none() -> Result<()> {
        let _env = env_lock::lock();
        let original_cwd = env::current_dir()?;
        let temp_dir = TempDir::new()?;
        env::set_current_dir(temp_dir.path())?;

        let resolved = resolve_workspace_path(None)?;
        assert_eq!(resolved, temp_dir.path().canonicalize()?);

        env::set_current_dir(original_cwd)?;
        Ok(())
    }

    #[test]
    fn resolves_relative_workspace_path() -> Result<()> {
        let _env = env_lock::lock();
        let original_cwd = env::current_dir()?;
        let temp_dir = TempDir::new()?;
        let workspace_dir = temp_dir.path().join("project");
        std::fs::create_dir(&workspace_dir)?;
        env::set_current_dir(temp_dir.path())?;

        let resolved = resolve_workspace_path(Some(PathBuf::from("project")))?;
        assert_eq!(resolved, workspace_dir.canonicalize()?);

        env::set_current_dir(original_cwd)?;
        Ok(())
    }

    #[test]
    fn parses_cli_config_entries_with_overrides() {
        let entries = vec![
            "agent.provider=openai".to_owned(),
            "custom-config/vtcode.toml".to_owned(),
        ];

        let (path, overrides) = parse_cli_config_entries(&entries);

        assert_eq!(path, Some(PathBuf::from("custom-config/vtcode.toml")));
        assert_eq!(
            overrides,
            vec![("agent.provider".to_owned(), "openai".to_owned())]
        );
    }

    #[test]
    fn cli_permission_overrides_append_unique_entries() {
        let mut config = VTCodeConfig::default();
        config.permissions.allow = vec!["read_file".to_string()];

        apply_cli_permission_overrides(
            &mut config,
            &[
                "read_file,unified_search".to_string(),
                "unified_exec".to_string(),
            ],
            &["apply_patch".to_string(), "unified_exec".to_string()],
        );

        assert_eq!(
            config.permissions.allow,
            vec![
                "read_file".to_string(),
                "unified_search".to_string(),
                "unified_exec".to_string()
            ]
        );
        assert_eq!(
            config.permissions.deny,
            vec!["apply_patch".to_string(), "unified_exec".to_string()]
        );
    }

    #[test]
    fn cli_permission_overrides_preserve_rule_shaped_entries() {
        let mut config = VTCodeConfig::default();

        apply_cli_permission_overrides(
            &mut config,
            &[
                "Read(/docs/**)".to_string(),
                "Bash(cargo check)".to_string(),
            ],
            &["Edit(/.git/**)".to_string()],
        );

        assert_eq!(
            config.permissions.allow,
            vec![
                "Read(/docs/**)".to_string(),
                "Bash(cargo check)".to_string()
            ]
        );
        assert_eq!(config.permissions.deny, vec!["Edit(/.git/**)".to_string()]);
    }

    #[test]
    fn applies_inline_overrides_to_config() -> Result<()> {
        let env_guard = env_lock::lock();
        let temp_dir = TempDir::new()?;
        let previous_config_dir = env::var_os("VTCODE_CONFIG");
        env_guard.set_var("VTCODE_CONFIG", temp_dir.path());

        let overrides = vec![("agent.provider".to_owned(), "\"openai\"".to_owned())];

        let manager = ConfigBuilder::new()
            .workspace(temp_dir.path().to_path_buf())
            .cli_overrides(&overrides)
            .build()?;
        let config = manager.config();

        assert_eq!(config.agent.provider, "openai");

        env_guard.restore_var("VTCODE_CONFIG", previous_config_dir);
        Ok(())
    }

    #[test]
    fn token_overhead_warnings_empty_for_default_config() {
        let config = VTCodeConfig::default();
        assert!(
            collect_token_overhead_warnings(&config).is_empty(),
            "default config should not trigger token-overhead warnings"
        );
    }

    #[test]
    fn token_overhead_warns_on_specialized_prompt_mode() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = vtcode_core::config::SystemPromptMode::Specialized;
        let warnings = collect_token_overhead_warnings(&config);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("system_prompt_mode = 'specialized'")),
            "expected a warning for specialized system prompt mode: {warnings:?}"
        );
    }

    #[test]
    fn token_overhead_warns_on_full_tool_docs() {
        let mut config = VTCodeConfig::default();
        config.agent.tool_documentation_mode = vtcode_core::config::ToolDocumentationMode::Full;
        let warnings = collect_token_overhead_warnings(&config);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("tool_documentation_mode = 'full'")),
            "expected a warning for full tool documentation mode: {warnings:?}"
        );
    }

    #[test]
    fn token_overhead_warns_on_disabled_tool_result_clearing() {
        let mut config = VTCodeConfig::default();
        config.agent.harness.tool_result_clearing.enabled = false;
        let warnings = collect_token_overhead_warnings(&config);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("tool_result_clearing is disabled")),
            "expected a warning for disabled tool-result clearing: {warnings:?}"
        );
    }

    #[test]
    fn token_overhead_warns_on_many_mcp_servers() {
        let mut config = VTCodeConfig::default();
        for _ in 0..12 {
            config.mcp.providers.push(McpProviderConfig::default());
        }
        let warnings = collect_token_overhead_warnings(&config);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("MCP servers")),
            "expected a warning for many MCP servers: {warnings:?}"
        );
    }
}
