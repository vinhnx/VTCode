use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::validator::ConfigValidator;
use vtcode_core::tools::RipgrepStatus;
use vtcode_core::utils::path::canonicalize_workspace;
use vtcode_core::utils::validation::validate_is_directory;

pub(super) fn apply_permission_mode_override(config: &mut VTCodeConfig, mode: &str) -> Result<()> {
    use vtcode_config::constants::tools;

    match mode.to_lowercase().as_str() {
        "ask" => {
            config.security.human_in_the_loop = true;
            config.security.require_write_tool_for_claims = true;
            config.automation.full_auto.enabled = false;
        }
        "suggest" => {
            config.security.human_in_the_loop = true;
            config.security.require_write_tool_for_claims = false;
            config.automation.full_auto.enabled = false;
        }
        "auto-approved" => {
            config.security.human_in_the_loop = false;
            config.security.require_write_tool_for_claims = false;
            config.automation.full_auto.enabled = true;
            config.automation.full_auto.allowed_tools = vec![
                tools::READ_FILE.to_string(),
                "list_files".to_string(),
                "grep_file".to_string(),
            ];
        }
        "full-auto" => {
            config.security.human_in_the_loop = false;
            config.security.require_write_tool_for_claims = false;
            config.automation.full_auto.enabled = true;
            config.automation.full_auto.allowed_tools = vec![];
        }
        "plan" => return Ok(()),
        _ => {
            bail!(
                "Invalid permission mode '{}'. Valid options: ask, suggest, auto-approved, full-auto, plan",
                mode
            );
        }
    }

    Ok(())
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

pub(super) fn validate_additional_directories(dirs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut validated_dirs = Vec::new();

    for dir in dirs {
        validate_is_directory(dir, "Additional directory")?;
        let absolute_dir = dir
            .canonicalize()
            .with_context(|| format!("Failed to resolve path '{}'", dir.display()))?;
        validated_dirs.push(absolute_dir);
    }

    Ok(validated_dirs)
}

pub(super) fn validate_full_auto_configuration(
    config: &VTCodeConfig,
    workspace: &Path,
) -> Result<()> {
    let automation_cfg = &config.automation.full_auto;
    if !automation_cfg.enabled {
        bail!(
            "Full-auto mode is disabled in configuration. Enable it under [automation.full_auto]."
        );
    }

    if automation_cfg.require_profile_ack {
        let profile_path = automation_cfg.profile_path.clone().ok_or_else(|| {
            anyhow!(
                "Full-auto mode requires 'profile_path' in [automation.full_auto] when require_profile_ack = true."
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
    workspace: &Path,
    quiet: bool,
) -> Result<()> {
    check_ripgrep_availability();

    let mut models_json_paths = vec![workspace.join("docs/models.json")];
    if let Ok(cwd) = std::env::current_dir() {
        models_json_paths.push(cwd.join("docs/models.json"));
    }

    let models_json_path = models_json_paths
        .iter()
        .find(|p| p.exists())
        .map(|p| p.to_path_buf());

    if let Some(models_path) = models_json_path {
        match ConfigValidator::new(&models_path) {
            Ok(validator) => {
                let _ = validator.validate(config);
            }
            Err(e) => {
                if !quiet {
                    eprintln!(
                        "vtcode: warning: could not load models database for validation: {}",
                        e
                    );
                }
            }
        }
    }

    Ok(())
}

fn check_ripgrep_availability() {
    match RipgrepStatus::check() {
        RipgrepStatus::Available { version } => {
            tracing::debug!("Ripgrep available: {}", version);
        }
        RipgrepStatus::NotFound => {
            tracing::debug!("Ripgrep not found; VT Code will use its built-in grep fallback");
        }
        RipgrepStatus::Error { reason } => {
            tracing::warn!("Ripgrep check error: {}", reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;
    use std::env;
    use std::sync::{Mutex, OnceLock};
    use vtcode_core::config::loader::ConfigBuilder;

    fn workspace_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("workspace lock")
    }

    #[test]
    fn resolves_current_dir_when_none() -> Result<()> {
        let _guard = workspace_guard();
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
        let _guard = workspace_guard();
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
    fn applies_inline_overrides_to_config() -> Result<()> {
        let _guard = workspace_guard();
        let temp_dir = TempDir::new()?;
        let previous_config_dir = env::var("VTCODE_CONFIG").ok();
        unsafe {
            env::set_var("VTCODE_CONFIG", temp_dir.path());
        }

        let overrides = vec![("agent.provider".to_owned(), "\"openai\"".to_owned())];

        let manager = ConfigBuilder::new()
            .workspace(temp_dir.path().to_path_buf())
            .cli_overrides(&overrides)
            .build()?;
        let config = manager.config();

        assert_eq!(config.agent.provider, "openai");

        unsafe {
            if let Some(previous) = previous_config_dir {
                env::set_var("VTCODE_CONFIG", previous);
            } else {
                env::remove_var("VTCODE_CONFIG");
            }
        }
        Ok(())
    }

    #[test]
    fn canonicalizes_additional_directories() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let extra_dir = temp_dir.path().join("extra");
        std::fs::create_dir(&extra_dir)?;

        let validated = validate_additional_directories(std::slice::from_ref(&extra_dir))?;

        assert_eq!(validated, vec![extra_dir.canonicalize()?]);
        Ok(())
    }
}
