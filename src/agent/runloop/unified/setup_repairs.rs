//! Setup self-repair: shared detection + apply for bypassed-wizard config.
//!
//! Single core behind every self-solve surface: the CLI `/doctor` repairs
//! section, the CLI `/config` "Diagnose & fix setup" entry, and the Desktop
//! diagnostics protocol methods. Detection is pure over the workspace and
//! loaded configuration; direct repairs mutate through [`ConfigService`] or
//! the bootstrap scaffold, while [`SetupRepairKind::GuidedSecret`] routes to
//! the surface's own secret flow (credentials never pass through here).
//!
//! Detection targets only genuine breakage or missing essentials: defaults
//! that work (empty provider/model) are not offered as repairs.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use vtcode_config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::{ConfigService, ConfigWriteRequest, ConfigWriteStrategy, ConfigWriteTarget};

pub const REPAIR_SCAFFOLD_WORKSPACE_CONFIG: &str = "scaffold_workspace_config";
pub const REPAIR_RESET_INVALID_CONFIG: &str = "reset_invalid_config";
pub const REPAIR_PICK_PROVIDER_MODEL: &str = "pick_provider_model";
pub const REPAIR_ADD_SECRET: &str = "add_secret";

pub const REPAIR_ALLOW_BUNDLED_SKILLS: &str = "allow_bundled_skills";
pub const REPAIR_DECLINE_BUNDLED_SKILLS: &str = "decline_bundled_skills";

/// One config write applied by a repair.
#[derive(Debug, Clone)]
pub struct RepairConfigEdit {
    pub target: ConfigWriteTarget,
    pub path: String,
    pub value: toml::Value,
}

/// How a repair is carried out.
#[derive(Debug, Clone)]
pub enum SetupRepairKind {
    /// Validated config writes, applied in order (stops at the first failure).
    SetConfig { edits: Vec<RepairConfigEdit> },
    /// Scaffold a missing workspace config from the bootstrap sample.
    ScaffoldWorkspaceConfig,
    /// Back up an unparseable workspace config, then scaffold fresh.
    ResetWorkspaceConfigWithBackup,
    /// Not applied here: the surface opens its provider/model picker, which
    /// owns selection, saving, and client rebuild.
    OpenModelPicker,
    /// Not applied here: the surface routes to its secret flow with these hints.
    GuidedSecret { provider: String, env_var: String },
}

/// One applicable setup repair.
#[derive(Debug, Clone)]
pub struct SetupRepair {
    pub id: &'static str,
    pub title: String,
    pub subtitle: String,
    pub search_value: String,
    pub kind: SetupRepairKind,
    /// Destructive repairs render with danger styling and an explicit apply step.
    pub destructive: bool,
    /// What applying would change. Only direct repairs carry one; routed
    /// repairs resolve interactively in their own UI.
    pub preview: SetupRepairPreview,
}

/// Preview of a repair's config changes, shown for confirmation before apply.
#[derive(Debug, Clone, Default)]
pub struct SetupRepairPreview {
    /// Where the change lands, human-readable (empty when not applicable).
    pub target_display: String,
    /// Per-key old → new values.
    pub changes: Vec<RepairPreviewChange>,
    /// Files a scaffold would create (missing files only).
    pub creates: Vec<String>,
    /// Extra context (backups, reversibility).
    pub note: Option<String>,
}

/// One key change inside a preview.
#[derive(Debug, Clone)]
pub struct RepairPreviewChange {
    pub path: String,
    pub old: String,
    pub new: String,
}

/// Detect applicable setup repairs for `workspace`.
///
/// Fails closed: an unreadable workspace or configuration yields no repairs
/// (the `/doctor` report already surfaces load failures), and an
/// unparseable workspace file yields only the reset repair so the remaining
/// detectors never misread defaults as user state. Broken user-layer files
/// are out of scope (`/config` reset covers them).
pub fn compute_setup_repairs(workspace: &Path) -> Vec<SetupRepair> {
    let workspace_file = workspace_file(workspace);
    // An explicit config override means the default workspace file is not in
    // play; file repairs would target the wrong file.
    let override_active = vtcode_config::loader::explicit_config_path().is_some();
    if !override_active
        && workspace_file_state(&workspace_file) == WorkspaceFileState::Present
        && workspace_config_is_broken(&workspace_file)
    {
        // No manager is loadable here by definition; the reset preview only
        // needs the workspace.
        let mut repair = reset_invalid_config_repair();
        repair.preview = reset_preview(workspace);
        return vec![repair];
    }

    let Ok(manager) = ConfigManager::load_from_workspace(workspace) else {
        return Vec::new();
    };

    let mut repairs = Vec::new();
    // Only truly unconfigured setups: a user-level config means the user
    // works without a workspace file on purpose.
    if !override_active
        && workspace_file_state(&workspace_file) == WorkspaceFileState::Missing
        && !manager.user_config_paths().iter().any(|path| path.is_file())
    {
        repairs.push(scaffold_workspace_config_repair());
    }
    repairs.extend(provider_model_repairs(&manager));
    repairs.extend(secret_repairs(&manager));
    repairs.extend(consent_repairs(&manager));
    let effective = toml::Value::try_from(manager.config()).ok();
    for repair in &mut repairs {
        repair.preview = build_preview(workspace, &manager, effective.as_ref(), repair);
    }
    repairs
}

/// Build what-applying-would-change for confirmation surfaces. Pure reads;
/// path resolution failures degrade to generic labels rather than failing.
fn build_preview(
    workspace: &Path,
    manager: &ConfigManager,
    effective: Option<&toml::Value>,
    repair: &SetupRepair,
) -> SetupRepairPreview {
    match &repair.kind {
        SetupRepairKind::SetConfig { edits } => {
            let target = edits.first().map(|edit| edit.target);
            debug_assert!(
                edits.iter().all(|edit| Some(edit.target) == target),
                "multi-target repairs need per-edit target display"
            );
            let target_display = target
                .map(|target| {
                    let label = match target {
                        ConfigWriteTarget::Workspace => "workspace",
                        ConfigWriteTarget::User => "user",
                        ConfigWriteTarget::Project => "project",
                    };
                    // Never guess a path: an unresolvable target shows the
                    // layer only, rather than a file that may be wrong.
                    ConfigService::resolve_target_path(workspace, &target)
                        .map(|path| {
                            format!(
                                "{} ({label})",
                                crate::agent::runloop::unified::settings_interactive::short_display_path(
                                    &path, workspace
                                )
                            )
                        })
                        .unwrap_or_else(|_| format!("({label} configuration)"))
                })
                .unwrap_or_default();
            let changes = edits
                .iter()
                .map(|edit| RepairPreviewChange {
                    path: edit.path.clone(),
                    old: effective_old_value(manager, effective, &edit.path),
                    new: toml_value_display(&edit.value),
                })
                .collect();
            SetupRepairPreview {
                target_display,
                changes,
                creates: Vec::new(),
                note: None,
            }
        }
        SetupRepairKind::ScaffoldWorkspaceConfig => scaffold_preview(workspace),
        SetupRepairKind::ResetWorkspaceConfigWithBackup => reset_preview(workspace),
        SetupRepairKind::OpenModelPicker | SetupRepairKind::GuidedSecret { .. } => SetupRepairPreview::default(),
    }
}

/// Effective old value for a dotted key: `(unset)` when no layer sets it.
fn effective_old_value(manager: &ConfigManager, effective: Option<&toml::Value>, path: &str) -> String {
    if !manager.has_explicit_dotted_key(path) {
        return "(unset)".to_string();
    }
    effective
        .and_then(|root| {
            let mut current = root;
            for segment in path.split('.') {
                current = current.as_table()?.get(segment)?;
            }
            Some(toml_value_display(current))
        })
        .unwrap_or_else(|| "(current value unavailable)".to_string())
}

fn toml_value_display(value: &toml::Value) -> String {
    let text = if let Some(text) = value.as_str() {
        text.to_string()
    } else {
        value.to_string()
    };
    // Previews render in terminals, reports, and desktop UI: never leak a
    // secret-shaped value, even if a future repair previews a sensitive key.
    vtcode_commons::sanitizer::redact_secrets(text)
}

/// Render a repair's preview as terminal lines, shared by the preview modal
/// and the headless report section.
#[must_use]
pub fn preview_lines(repair: &SetupRepair) -> Vec<String> {
    let mut lines = Vec::new();
    if !repair.preview.target_display.is_empty() {
        lines.push(format!("In {}", repair.preview.target_display));
    }
    for change in &repair.preview.changes {
        lines.push(format!("`{}`: {} → {}", change.path, change.old, change.new));
    }
    for create in &repair.preview.creates {
        lines.push(format!("+ creates `{create}`"));
    }
    if let Some(note) = repair.preview.note.as_deref() {
        lines.push(note.to_string());
    }
    lines
}

fn scaffold_preview(workspace: &Path) -> SetupRepairPreview {
    let candidates = [
        workspace.join("vtcode.toml"),
        workspace.join(".gitignore"),
        workspace.join(".vtcode/README.md"),
    ];
    let creates = candidates
        .iter()
        .filter(|path| !path.exists())
        .map(|path| crate::agent::runloop::unified::settings_interactive::short_display_path(path, workspace))
        .collect();
    SetupRepairPreview {
        target_display: String::new(),
        changes: Vec::new(),
        creates,
        note: Some("Existing files are left untouched.".to_string()),
    }
}

fn reset_preview(workspace: &Path) -> SetupRepairPreview {
    let lines = std::fs::read_to_string(workspace_file(workspace))
        .map(|content| content.lines().count())
        .unwrap_or(0);
    SetupRepairPreview {
        target_display: crate::agent::runloop::unified::settings_interactive::short_display_path(
            &workspace_file(workspace),
            workspace,
        ),
        changes: Vec::new(),
        creates: Vec::new(),
        note: Some(format!(
            "Current file is unparseable ({lines} lines). Original is backed up alongside vtcode.toml first."
        )),
    }
}

/// Whether `provider_key` has a usable credential by the same rule the
/// runtime uses: a resolvable key (covers custom providers via their env
/// vars, and OpenRouter OAuth whose token carries a key), or — for OpenAI —
/// whatever `vtcode_auth::summarize_openai_credentials` reports active,
/// including the Codex fallback. Sharing the runtime rule (not reimplementing
/// it) keeps detection and the `/doctor` verdict in lockstep.
fn provider_has_credential(
    provider_key: &str,
    storage_mode: vtcode_config::auth::AuthCredentialsStoreMode,
    auth_config: &vtcode_config::core::OpenAIAuthConfig,
) -> bool {
    use vtcode_config::api_keys::{ApiKeySources, get_api_key_with_mode};

    if get_api_key_with_mode(provider_key, &ApiKeySources::default(), storage_mode).is_ok() {
        return true;
    }
    if !provider_key.trim().eq_ignore_ascii_case("openai") {
        return false;
    }
    vtcode_auth::openai_chatgpt_oauth::summarize_openai_credentials(auth_config, storage_mode, None)
        .map(|overview| overview.active_source.is_some())
        .unwrap_or(false)
}

/// Apply a directly-appliable repair. Guided repairs must be routed by the
/// surface instead; calling this with one is a programmer error.
pub fn apply_setup_repair(workspace: &Path, repair: &SetupRepair) -> Result<String> {
    match &repair.kind {
        SetupRepairKind::SetConfig { edits } => {
            for edit in edits {
                ConfigService::write(ConfigWriteRequest {
                    workspace: workspace.to_path_buf(),
                    target: edit.target,
                    path: edit.path.clone(),
                    value: edit.value.clone(),
                    strategy: ConfigWriteStrategy::Replace,
                    expected_layer_version: None,
                })
                .with_context(|| format!("Failed to write {}", edit.path))?;
            }
            Ok(format!("Applied '{}'.", repair.title))
        }
        SetupRepairKind::ScaffoldWorkspaceConfig => {
            let created = VTCodeConfig::bootstrap_project(workspace, false)
                .context("Failed to scaffold workspace configuration")?;
            Ok(format!("Scaffolded workspace configuration ({}).", created.join(", ")))
        }
        SetupRepairKind::ResetWorkspaceConfigWithBackup => {
            let backup = backup_workspace_config(workspace)?;
            VTCodeConfig::create_sample_config(workspace_file(workspace))
                .context("Failed to write fresh workspace configuration")?;
            Ok(format!("Backed up broken config to {} and wrote fresh defaults.", backup.display()))
        }
        SetupRepairKind::OpenModelPicker => {
            bail!("'{}' needs the model picker, which owns selection and client rebuild.", repair.title)
        }
        SetupRepairKind::GuidedSecret { provider, env_var } => {
            bail!("'{}' needs the secret flow: store a key for {provider} ({env_var}).", repair.title)
        }
    }
}

fn workspace_config_is_broken(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    toml::from_str::<toml::Value>(&content).is_err()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceFileState {
    Missing,
    Present,
    /// Never repaired: writes would follow the link outside the workspace.
    Symlink,
}

fn workspace_file_state(path: &Path) -> WorkspaceFileState {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) => return WorkspaceFileState::Missing,
    };
    if meta.file_type().is_symlink() {
        WorkspaceFileState::Symlink
    } else if meta.is_file() {
        WorkspaceFileState::Present
    } else {
        WorkspaceFileState::Missing
    }
}

fn reset_invalid_config_repair() -> SetupRepair {
    SetupRepair {
        id: REPAIR_RESET_INVALID_CONFIG,
        title: "Reset broken workspace config".to_string(),
        subtitle: "vtcode.toml does not parse. Back it up and write fresh defaults.".to_string(),
        search_value: "repair reset broken invalid config toml backup".to_string(),
        kind: SetupRepairKind::ResetWorkspaceConfigWithBackup,
        destructive: true,
        preview: SetupRepairPreview::default(),
    }
}

fn scaffold_workspace_config_repair() -> SetupRepair {
    SetupRepair {
        id: REPAIR_SCAFFOLD_WORKSPACE_CONFIG,
        title: "Create workspace config".to_string(),
        subtitle: "No vtcode.toml anywhere. Scaffold workspace defaults (same as setup).".to_string(),
        search_value: "repair scaffold create workspace config init".to_string(),
        kind: SetupRepairKind::ScaffoldWorkspaceConfig,
        destructive: false,
        preview: SetupRepairPreview::default(),
    }
}

/// At most one picker repair: an unknown provider, an invalid model, or both.
/// Routing (not writing) keeps selection, saving, and client rebuild inside
/// the surfaces' existing pickers.
fn provider_model_repairs(manager: &ConfigManager) -> Vec<SetupRepair> {
    let config = manager.config();
    if provider_uses_managed_auth(&config.agent.provider) {
        return Vec::new();
    }
    let mut reasons = Vec::new();
    if crate::startup::validate_runtime_provider(config, &config.agent.provider).is_err() {
        reasons.push(format!("provider '{}' is unknown", config.agent.provider));
    }
    let model = config.agent.default_model.trim();
    if !model.is_empty()
        && vtcode_core::config::validation::validate_model_exists(&config.agent.provider, model).is_err()
    {
        reasons.push(format!("model '{model}' is not valid for the active provider"));
    }
    if reasons.is_empty() {
        return Vec::new();
    }
    Vec::from([SetupRepair {
        id: REPAIR_PICK_PROVIDER_MODEL,
        title: "Choose provider & model".to_string(),
        subtitle: format!("{}.", reasons.join("; ")),
        search_value: "repair provider model picker invalid unknown choose".to_string(),
        kind: SetupRepairKind::OpenModelPicker,
        destructive: false,
        preview: SetupRepairPreview::default(),
    }])
}

fn secret_repairs(manager: &ConfigManager) -> Vec<SetupRepair> {
    let config = manager.config();
    if crate::startup::validate_runtime_provider(config, &config.agent.provider).is_err() {
        return Vec::new();
    }
    if provider_uses_managed_auth(&config.agent.provider) {
        return Vec::new();
    }
    if !super::diagnostics::provider_requires_api_key(&config.agent.provider) {
        return Vec::new();
    }
    if provider_has_credential(&config.agent.provider, config.agent.credential_storage_mode, &config.auth.openai) {
        return Vec::new();
    }
    Vec::from([guided_secret_repair(&config.agent.provider, &secret_env_hint(manager))])
}

/// Providers whose credentials live outside VT Code (companion CLIs and
/// servers): secret and picker repairs cannot help — their login flows own
/// auth, so no repair is offered.
fn provider_uses_managed_auth(provider_key: &str) -> bool {
    use std::str::FromStr;
    provider_key
        .trim()
        .eq_ignore_ascii_case(crate::codex_app_server::CODEX_PROVIDER)
        || vtcode_config::models::Provider::from_str(provider_key.trim())
            .is_ok_and(|provider| provider.uses_managed_auth())
}

/// Env var to name in secret guidance: the user's explicit override when set,
/// otherwise the effective provider's default (a stale default from a
/// provider switch would otherwise misdirect the user).
fn secret_env_hint(manager: &ConfigManager) -> String {
    use std::str::FromStr;
    let config = manager.config();
    if manager.has_explicit_dotted_key("agent.api_key_env") {
        return config.agent.api_key_env.clone();
    }
    vtcode_config::models::Provider::from_str(config.agent.provider.trim())
        .map(|provider| provider.default_api_key_env().to_string())
        .unwrap_or_else(|_| config.agent.api_key_env.clone())
}

/// Pure constructor for the guided secret repair. Separated so tests pin the
/// shape without touching live credential stores.
fn guided_secret_repair(provider_label: &str, env_var: &str) -> SetupRepair {
    SetupRepair {
        id: REPAIR_ADD_SECRET,
        title: format!("Add {provider_label} API key"),
        subtitle: format!("Store {env_var} via the secret flow (keys are never typed here)."),
        search_value: format!("repair secret api key {provider_label} add"),
        kind: SetupRepairKind::GuidedSecret {
            provider: provider_label.to_string(),
            env_var: env_var.to_string(),
        },
        destructive: false,
        preview: SetupRepairPreview::default(),
    }
}

fn consent_repairs(manager: &ConfigManager) -> Vec<SetupRepair> {
    if vtcode_config::consent::bundled_skills_answer(manager) != vtcode_config::consent::ConsentAnswer::Unasked {
        return Vec::new();
    }
    if vtcode_core::skills::system::bundled_skills_grandfathered() {
        return Vec::new();
    }
    Vec::from([
        SetupRepair {
            id: REPAIR_ALLOW_BUNDLED_SKILLS,
            title: "Allow bundled skills".to_string(),
            subtitle: "Install cmd-review, skill-creator, skill-installer (recommended).".to_string(),
            search_value: "repair consent allow bundled skills install".to_string(),
            kind: SetupRepairKind::SetConfig {
                edits: Vec::from([RepairConfigEdit {
                    target: ConfigWriteTarget::User,
                    path: vtcode_config::consent::BUNDLED_SKILLS_SETTING_PATH.to_string(),
                    value: toml::Value::Boolean(true),
                }]),
            },
            destructive: false,
            preview: SetupRepairPreview::default(),
        },
        SetupRepair {
            id: REPAIR_DECLINE_BUNDLED_SKILLS,
            title: "Decline bundled skills".to_string(),
            subtitle: "Skip the install. VT Code works without these skills.".to_string(),
            search_value: "repair consent decline bundled skills skip".to_string(),
            kind: SetupRepairKind::SetConfig {
                edits: Vec::from([RepairConfigEdit {
                    target: ConfigWriteTarget::User,
                    path: vtcode_config::consent::BUNDLED_SKILLS_SETTING_PATH.to_string(),
                    value: toml::Value::Boolean(false),
                }]),
            },
            destructive: false,
            preview: SetupRepairPreview::default(),
        },
    ])
}

fn backup_workspace_config(workspace: &Path) -> Result<PathBuf> {
    let source = workspace_file(workspace);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let backup = workspace.join(format!("vtcode.toml.broken.{stamp}.bak"));
    let content = std::fs::read(&source).with_context(|| format!("Failed to read {}", source.display()))?;
    vtcode_commons::VtCodePaths::write_private_file_atomic(&backup, &content)
        .with_context(|| format!("Failed to back up broken config to {}", backup.display()))?;
    Ok(backup)
}

fn workspace_file(workspace: &Path) -> PathBuf {
    workspace.join("vtcode.toml")
}

#[cfg(test)]
mod tests;
