use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Result;
use tracing::{debug, warn};
use vtcode_config::IdeContextConfig;
use vtcode_core::ide_context::{
    EditorContextSnapshot, IDE_CONTEXT_ENV_VAR, LEGACY_VSCODE_CONTEXT_ENV_VAR,
};
use vtcode_core::tools::dominant_workspace_language;

const WORKSPACE_IDE_CONTEXT_JSON_FILE: &str = ".vtcode/ide-context.json";
const WORKSPACE_IDE_CONTEXT_MARKDOWN_FILE: &str = ".vtcode/ide-context.md";
const VSCODE_COMPATIBLE_STORAGE_DIRS: &[&str] = &[
    "Code",
    "Code - Insiders",
    "Cursor",
    "Windsurf",
    "VSCodium",
    "Kiro",
];
const VSCODE_COMPATIBLE_EXTENSION_ID: &str = "nguyenxuanvinh.vtcode-companion";
const VSCODE_COMPATIBLE_JSON_FILE: &str = "vtcode-ide-context.json";
const VSCODE_COMPATIBLE_MARKDOWN_FILE: &str = "vtcode-ide-context.md";

pub(crate) struct IdeContextBridge {
    workspace_root: PathBuf,
    last_digest: Option<u64>,
    snapshot: Option<EditorContextSnapshot>,
    source: Option<PathBuf>,
}

impl IdeContextBridge {
    pub(crate) fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            last_digest: None,
            snapshot: None,
            source: None,
        }
    }

    pub(crate) fn refresh(
        &mut self,
    ) -> Result<(Option<EditorContextSnapshot>, IdeContextRefreshState)> {
        let resolved = read_current_ide_context_snapshot(&self.workspace_root)?;
        let snapshot = resolved.snapshot;
        let digest = compute_snapshot_digest(snapshot.as_ref());
        let source = resolved.source;
        let changed = self.last_digest != digest || self.source != source;

        if changed {
            self.last_digest = digest;
            self.snapshot = snapshot;
            self.source = source;
            log_refresh_result(
                &self.workspace_root,
                self.source.as_deref(),
                self.snapshot.as_ref(),
            );
        }

        Ok((
            self.snapshot.clone(),
            IdeContextRefreshState {
                changed,
                available: self.snapshot.is_some(),
            },
        ))
    }

    pub(crate) fn snapshot(&self) -> Option<&EditorContextSnapshot> {
        self.snapshot.as_ref()
    }

    pub(crate) fn snapshot_source(&self) -> Option<&Path> {
        self.source.as_deref()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct IdeContextRefreshState {
    pub(crate) changed: bool,
    pub(crate) available: bool,
}

pub(crate) fn preferred_display_language_for_workspace(workspace: &Path) -> Option<String> {
    let snapshot = match read_current_ide_context_snapshot(workspace) {
        Ok(resolved) => resolved.snapshot,
        Err(error) => {
            warn!(
                workspace = %workspace.display(),
                error = ?error,
                "Failed to read IDE context while resolving active editor language"
            );
            None
        }
    };

    preferred_display_language_for_workspace_with_snapshot(workspace, snapshot.as_ref())
}

pub(crate) fn tui_header_summary(
    workspace: &Path,
    config: Option<&IdeContextConfig>,
    snapshot: Option<&EditorContextSnapshot>,
) -> Option<String> {
    let snapshot = configured_snapshot(config, snapshot);
    let Some(cfg) = config else {
        return snapshot.and_then(|current| current.header_summary(workspace));
    };

    if !cfg.enabled || !cfg.show_in_tui {
        return None;
    }

    snapshot.and_then(|current| current.header_summary(workspace))
}

pub(crate) fn configured_snapshot<'a>(
    config: Option<&IdeContextConfig>,
    snapshot: Option<&'a EditorContextSnapshot>,
) -> Option<&'a EditorContextSnapshot> {
    let snapshot = snapshot?;
    if config.is_some_and(|cfg| !cfg.allows_provider_family(snapshot.provider_family)) {
        return None;
    }

    Some(snapshot)
}

pub(crate) fn status_line_editor_label(
    workspace: &Path,
    config: Option<&IdeContextConfig>,
    snapshot: Option<&EditorContextSnapshot>,
    source: Option<&Path>,
) -> Option<String> {
    let show_in_tui = config.is_none_or(|cfg| cfg.enabled && cfg.show_in_tui);
    if !show_in_tui {
        return None;
    }

    let snapshot = configured_snapshot(config, snapshot);
    let prefix = compact_tui_label_prefix(snapshot, source);

    if let Some(snapshot) = snapshot
        && let Some(file) = snapshot.active_file.as_ref()
    {
        let path = file.display_path(workspace, snapshot.workspace_root.as_deref());
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(format_status_line_ide_context_label(&prefix, trimmed));
        }
    }

    display_ide_context_source(workspace, source)
        .map(|compact| format_status_line_ide_context_label(&prefix, &compact))
}

fn compact_tui_label_prefix(
    snapshot: Option<&EditorContextSnapshot>,
    source: Option<&Path>,
) -> String {
    snapshot
        .and_then(|snapshot| snapshot.editor_name.as_deref())
        .and_then(normalize_ide_display_name)
        .map(ToOwned::to_owned)
        .or_else(|| infer_ide_display_name_from_source(source).map(ToOwned::to_owned))
        .unwrap_or_else(|| "IDE".to_string())
}

fn normalize_ide_display_name(name: &str) -> Option<&'static str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.to_ascii_lowercase();
    if normalized.contains("cursor") {
        Some("Cursor")
    } else if normalized.contains("kiro") {
        Some("Kiro")
    } else if normalized.contains("windsurf") {
        Some("Windsurf")
    } else if normalized.contains("vscodium") {
        Some("VSCodium")
    } else if normalized.contains("vs code insiders")
        || normalized.contains("visual studio code insiders")
        || normalized.contains("code - insiders")
        || normalized.contains("insider")
    {
        Some("VS Code Insiders")
    } else if normalized.contains("vs code")
        || normalized.contains("visual studio code")
        || normalized == "code"
    {
        Some("VS Code")
    } else {
        None
    }
}

fn infer_ide_display_name_from_source(source: Option<&Path>) -> Option<&'static str> {
    let source = source?;
    source
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .find_map(normalize_ide_display_name)
}

fn format_status_line_ide_context_label(prefix: &str, context: &str) -> String {
    format!("IDE Context ({prefix}): {}", context.trim())
}

fn display_ide_context_source(workspace: &Path, source: Option<&Path>) -> Option<String> {
    let source = source?;
    let compact = source
        .strip_prefix(workspace)
        .ok()
        .map(|path| path.display().to_string())
        .or_else(|| {
            source
                .file_name()
                .map(|name| name.to_string_lossy().trim().to_string())
                .filter(|name| !name.is_empty())
        })
        .unwrap_or_else(|| source.display().to_string());

    let compact = compact.trim();
    if compact.is_empty() {
        None
    } else {
        Some(compact.to_string())
    }
}

fn preferred_display_language_for_workspace_with_snapshot(
    workspace: &Path,
    snapshot: Option<&EditorContextSnapshot>,
) -> Option<String> {
    snapshot
        .and_then(EditorContextSnapshot::active_display_language)
        .or_else(|| dominant_workspace_language(workspace))
}

#[derive(Debug, Default)]
struct ResolvedIdeContextSnapshot {
    snapshot: Option<EditorContextSnapshot>,
    source: Option<PathBuf>,
}

fn read_current_ide_context_snapshot(workspace: &Path) -> Result<ResolvedIdeContextSnapshot> {
    if let Some((snapshot, source)) = read_env_ide_context_snapshot()? {
        return Ok(ResolvedIdeContextSnapshot {
            snapshot: Some(snapshot),
            source: Some(source),
        });
    }

    if let Some((snapshot, source)) = read_workspace_ide_context_snapshot(workspace)? {
        return Ok(ResolvedIdeContextSnapshot {
            snapshot: Some(snapshot),
            source: Some(source),
        });
    }

    if let Some((snapshot, source)) = read_vscode_compatible_global_storage_snapshot(workspace) {
        return Ok(ResolvedIdeContextSnapshot {
            snapshot: Some(snapshot),
            source: Some(source),
        });
    }

    Ok(ResolvedIdeContextSnapshot::default())
}

fn read_env_ide_context_snapshot() -> Result<Option<(EditorContextSnapshot, PathBuf)>> {
    if let Some(path) = snapshot_path_from_env(IDE_CONTEXT_ENV_VAR)
        && let Some(snapshot) = EditorContextSnapshot::read_json_file(&path)?
    {
        return Ok(Some((snapshot, path)));
    }

    if let Some(path) = snapshot_path_from_env(LEGACY_VSCODE_CONTEXT_ENV_VAR)
        && let Some(snapshot) = EditorContextSnapshot::read_legacy_markdown_file(&path)?
    {
        return Ok(Some((snapshot, path)));
    }

    Ok(None)
}

fn read_workspace_ide_context_snapshot(
    workspace: &Path,
) -> Result<Option<(EditorContextSnapshot, PathBuf)>> {
    let json_path = workspace.join(WORKSPACE_IDE_CONTEXT_JSON_FILE);
    if let Some(snapshot) = EditorContextSnapshot::read_json_file(&json_path)? {
        return Ok(Some((snapshot, json_path)));
    }

    let legacy_markdown_path = workspace.join(WORKSPACE_IDE_CONTEXT_MARKDOWN_FILE);
    Ok(
        EditorContextSnapshot::read_legacy_markdown_file(&legacy_markdown_path)?
            .map(|snapshot| (snapshot, legacy_markdown_path)),
    )
}

fn read_vscode_compatible_global_storage_snapshot(
    workspace: &Path,
) -> Option<(EditorContextSnapshot, PathBuf)> {
    let roots = vscode_compatible_global_storage_roots();
    read_vscode_compatible_snapshot_from_roots(workspace, &roots)
}

fn read_vscode_compatible_snapshot_from_roots(
    workspace: &Path,
    roots: &[PathBuf],
) -> Option<(EditorContextSnapshot, PathBuf)> {
    for root in roots {
        let json_path = root.join(VSCODE_COMPATIBLE_JSON_FILE);
        if let Some(snapshot) = try_read_discovered_snapshot(workspace, &json_path, true) {
            return Some((snapshot, json_path));
        }

        let markdown_path = root.join(VSCODE_COMPATIBLE_MARKDOWN_FILE);
        if let Some(snapshot) = try_read_discovered_snapshot(workspace, &markdown_path, false) {
            return Some((snapshot, markdown_path));
        }
    }

    None
}

fn try_read_discovered_snapshot(
    workspace: &Path,
    path: &Path,
    is_json: bool,
) -> Option<EditorContextSnapshot> {
    if !path.is_file() {
        return None;
    }

    let read_result = if is_json {
        EditorContextSnapshot::read_json_file(path)
    } else {
        EditorContextSnapshot::read_legacy_markdown_file(path)
    };

    match read_result {
        Ok(Some(snapshot)) if snapshot_matches_workspace(workspace, &snapshot) => Some(snapshot),
        Ok(Some(_)) => None,
        Ok(None) => None,
        Err(error) => {
            warn!(
                workspace = %workspace.display(),
                path = %path.display(),
                error = ?error,
                "Failed to read discovered IDE context snapshot"
            );
            None
        }
    }
}

fn snapshot_matches_workspace(workspace: &Path, snapshot: &EditorContextSnapshot) -> bool {
    snapshot
        .workspace_root
        .as_deref()
        .is_some_and(|root| paths_match(root, workspace))
        || snapshot
            .active_file
            .as_ref()
            .is_some_and(|file| file_path_matches_workspace(workspace, &file.path))
        || snapshot
            .visible_editors
            .iter()
            .any(|file| file_path_matches_workspace(workspace, &file.path))
}

fn file_path_matches_workspace(workspace: &Path, raw_path: &str) -> bool {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() || trimmed.contains("://") || trimmed.starts_with("untitled:") {
        return false;
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        return path.starts_with(workspace);
    }

    workspace.join(path).exists()
}

fn paths_match(left: &Path, right: &Path) -> bool {
    left == right
        || fs::canonicalize(left)
            .ok()
            .zip(fs::canonicalize(right).ok())
            .is_some_and(|(left, right)| left == right)
}

fn vscode_compatible_global_storage_roots() -> Vec<PathBuf> {
    let Some(config_dir) = dirs::config_dir() else {
        return Vec::new();
    };

    VSCODE_COMPATIBLE_STORAGE_DIRS
        .iter()
        .map(|app_dir| {
            config_dir
                .join(app_dir)
                .join("User")
                .join("globalStorage")
                .join(VSCODE_COMPATIBLE_EXTENSION_ID)
        })
        .collect()
}

fn snapshot_path_from_env(env_var: &str) -> Option<PathBuf> {
    env::var_os(env_var)
        .map(|value| value.to_string_lossy().trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn log_refresh_result(
    workspace: &Path,
    source: Option<&Path>,
    snapshot: Option<&EditorContextSnapshot>,
) {
    let Some(snapshot) = snapshot else {
        debug!(
            workspace = %workspace.display(),
            "No IDE context snapshot available"
        );
        return;
    };

    let selection = snapshot.active_file.as_ref().and_then(|file| {
        file.selection.as_ref().map(|selection| {
            format!(
                "{}:{}-{}:{}",
                selection.range.start_line,
                selection.range.start_column,
                selection.range.end_line,
                selection.range.end_column
            )
        })
    });

    debug!(
        workspace = %workspace.display(),
        source = %source
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unknown>".to_string()),
        provider_family = ?snapshot.provider_family,
        active_file = ?snapshot.active_file.as_ref().map(|file| file.path.as_str()),
        selection = ?selection,
        "Loaded IDE context snapshot"
    );
}

fn compute_snapshot_digest(snapshot: Option<&EditorContextSnapshot>) -> Option<u64> {
    snapshot.map(|value| {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    })
}

#[cfg(test)]
mod tests {
    use super::{
        IdeContextBridge, VSCODE_COMPATIBLE_JSON_FILE, configured_snapshot,
        preferred_display_language_for_workspace,
        preferred_display_language_for_workspace_with_snapshot,
        read_vscode_compatible_snapshot_from_roots, status_line_editor_label, tui_header_summary,
    };
    use serial_test::serial;
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;
    use vtcode_config::IdeContextConfig;
    use vtcode_core::ide_context::{EditorContextSnapshot, IDE_CONTEXT_ENV_VAR};

    #[test]
    fn preferred_language_uses_snapshot_before_workspace_fallback() {
        let workspace = TempDir::new().expect("workspace tempdir");
        fs::create_dir_all(workspace.path().join("src")).expect("create src");
        fs::write(workspace.path().join("src/lib.rs"), "fn alpha() {}\n").expect("write rust");

        let snapshot = EditorContextSnapshot {
            active_file: Some(vtcode_core::EditorFileContext {
                path: workspace.path().join("main.py").display().to_string(),
                language_id: Some("python".to_string()),
                line_range: None,
                dirty: false,
                truncated: false,
                selection: None,
            }),
            ..EditorContextSnapshot::default()
        };

        assert_eq!(
            preferred_display_language_for_workspace_with_snapshot(
                workspace.path(),
                Some(&snapshot)
            ),
            Some("Python".to_string())
        );
    }

    #[test]
    fn preferred_language_falls_back_to_workspace_when_snapshot_missing() {
        let workspace = TempDir::new().expect("workspace tempdir");
        fs::create_dir_all(workspace.path().join("src")).expect("create src");
        fs::write(workspace.path().join("src/lib.rs"), "fn alpha() {}\n").expect("write rust");

        assert_eq!(
            preferred_display_language_for_workspace_with_snapshot(workspace.path(), None),
            Some("Rust".to_string())
        );
    }

    #[test]
    fn tui_header_summary_respects_visibility_toggle() {
        let snapshot = EditorContextSnapshot {
            workspace_root: Some(PathBuf::from("/workspace")),
            active_file: Some(vtcode_core::EditorFileContext {
                path: "/workspace/src/main.rs".to_string(),
                language_id: Some("rust".to_string()),
                line_range: None,
                dirty: false,
                truncated: false,
                selection: None,
            }),
            ..EditorContextSnapshot::default()
        };

        let config = IdeContextConfig {
            show_in_tui: false,
            ..IdeContextConfig::default()
        };

        assert_eq!(
            tui_header_summary(Path::new("/workspace"), Some(&config), Some(&snapshot)),
            None
        );
    }

    #[test]
    #[serial]
    fn bridge_refresh_detects_snapshot_changes() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("snapshot.json");
        fs::write(
            &path,
            r#"{
                "version": 1,
                "provider_family": "zed",
                "workspace_root": "/workspace",
                "active_file": {
                    "path": "/workspace/src/main.rs",
                    "language_id": "rust",
                    "dirty": false,
                    "truncated": false
                }
            }"#,
        )
        .expect("write snapshot");

        unsafe {
            env::set_var(IDE_CONTEXT_ENV_VAR, &path);
        }

        let mut bridge = IdeContextBridge::new(temp.path());
        let (_, first_state) = bridge.refresh().expect("refresh");
        assert!(first_state.changed);
        assert!(bridge.snapshot().is_some());

        let (_, second_state) = bridge.refresh().expect("refresh");
        assert!(!second_state.changed);

        unsafe {
            env::remove_var(IDE_CONTEXT_ENV_VAR);
        }
    }

    #[test]
    #[serial]
    fn bridge_refresh_reads_workspace_snapshot_without_env() {
        let workspace = TempDir::new().expect("workspace tempdir");
        let snapshot_dir = workspace.path().join(".vtcode");
        fs::create_dir_all(&snapshot_dir).expect("create snapshot dir");
        fs::write(
            snapshot_dir.join("ide-context.json"),
            r#"{
                "version": 1,
                "provider_family": "vscode_compatible",
                "workspace_root": "/workspace",
                "active_file": {
                    "path": "/workspace/src/main.rs",
                    "language_id": "rust",
                    "dirty": false,
                    "truncated": false
                }
            }"#,
        )
        .expect("write workspace snapshot");

        unsafe {
            env::remove_var(IDE_CONTEXT_ENV_VAR);
        }

        let mut bridge = IdeContextBridge::new(workspace.path());
        let (_, state) = bridge.refresh().expect("refresh");

        assert!(state.changed);
        assert!(state.available);
        assert_eq!(
            bridge
                .snapshot()
                .and_then(EditorContextSnapshot::active_display_language),
            Some("Rust".to_string())
        );
    }

    #[test]
    fn configured_snapshot_respects_provider_family_filters() {
        let snapshot = EditorContextSnapshot {
            provider_family: vtcode_config::IdeContextProviderFamily::Zed,
            ..EditorContextSnapshot::default()
        };
        let config = IdeContextConfig {
            provider_mode: vtcode_config::IdeContextProviderMode::VscodeCompatible,
            ..IdeContextConfig::default()
        };

        assert!(configured_snapshot(Some(&config), Some(&snapshot)).is_none());
        assert!(configured_snapshot(None, Some(&snapshot)).is_some());
    }

    #[test]
    fn status_line_editor_label_prefers_active_file_display_path() {
        let workspace = Path::new("/workspace");
        let snapshot = EditorContextSnapshot {
            editor_name: Some("VS Code".to_string()),
            workspace_root: Some(PathBuf::from("/workspace")),
            active_file: Some(vtcode_core::EditorFileContext {
                path: "/workspace/vtcode-config/src/core/agent.rs".to_string(),
                language_id: Some("rust".to_string()),
                line_range: None,
                dirty: false,
                truncated: false,
                selection: None,
            }),
            ..EditorContextSnapshot::default()
        };

        assert_eq!(
            status_line_editor_label(workspace, None, Some(&snapshot), None).as_deref(),
            Some("IDE Context (VS Code): vtcode-config/src/core/agent.rs")
        );
    }

    #[test]
    fn status_line_editor_label_falls_back_to_source_when_snapshot_has_no_active_file() {
        let workspace = Path::new("/workspace");
        let snapshot = EditorContextSnapshot {
            editor_name: Some("Cursor".to_string()),
            ..EditorContextSnapshot::default()
        };
        let source = Path::new("/workspace/.vtcode/ide-context.json");

        assert_eq!(
            status_line_editor_label(
                workspace,
                Some(&IdeContextConfig::default()),
                Some(&snapshot),
                Some(source),
            )
            .as_deref(),
            Some("IDE Context (Cursor): .vtcode/ide-context.json")
        );
    }

    #[test]
    fn reads_matching_vscode_compatible_global_storage_snapshot() {
        let workspace = TempDir::new().expect("workspace tempdir");
        let storage_root = TempDir::new().expect("storage root");
        let snapshot_path = storage_root.path().join(VSCODE_COMPATIBLE_JSON_FILE);
        fs::write(
            &snapshot_path,
            format!(
                r#"{{
                    "version": 1,
                    "provider_family": "vscode_compatible",
                    "workspace_root": "{}",
                    "active_file": {{
                        "path": "{}/docs/project/TODO.md",
                        "language_id": "markdown",
                        "dirty": false,
                        "truncated": false,
                        "selection": {{
                            "range": {{
                                "start_line": 1435,
                                "start_column": 1,
                                "end_line": 1435,
                                "end_column": 58
                            }},
                            "text": "To more harness building, better systems, and better agents."
                        }}
                    }}
                }}"#,
                workspace.path().display(),
                workspace.path().display()
            ),
        )
        .expect("write snapshot");

        let (snapshot, source) = read_vscode_compatible_snapshot_from_roots(
            workspace.path(),
            &[storage_root.path().to_path_buf()],
        )
        .expect("snapshot");

        let expected_path = format!("{}/docs/project/TODO.md", workspace.path().display());
        assert_eq!(source, snapshot_path);
        assert_eq!(
            snapshot.active_file.as_ref().map(|file| file.path.as_str()),
            Some(expected_path.as_str())
        );
        assert!(snapshot.has_explicit_selection());
    }

    #[test]
    fn ignores_vscode_compatible_global_storage_snapshot_for_other_workspace() {
        let workspace = TempDir::new().expect("workspace tempdir");
        let other_workspace = TempDir::new().expect("other workspace");
        let storage_root = TempDir::new().expect("storage root");
        let snapshot_path = storage_root.path().join(VSCODE_COMPATIBLE_JSON_FILE);
        fs::write(
            &snapshot_path,
            format!(
                r#"{{
                    "version": 1,
                    "provider_family": "vscode_compatible",
                    "workspace_root": "{}",
                    "active_file": {{
                        "path": "{}/docs/project/TODO.md",
                        "language_id": "markdown",
                        "dirty": false,
                        "truncated": false
                    }}
                }}"#,
                other_workspace.path().display(),
                other_workspace.path().display()
            ),
        )
        .expect("write snapshot");

        assert!(
            read_vscode_compatible_snapshot_from_roots(
                workspace.path(),
                &[storage_root.path().to_path_buf()],
            )
            .is_none()
        );
    }

    #[test]
    #[serial]
    fn preferred_language_reads_current_snapshot_from_env() {
        let workspace = TempDir::new().expect("workspace tempdir");
        let path = workspace.path().join("snapshot.json");
        fs::write(
            &path,
            r#"{
                "version": 1,
                "provider_family": "generic",
                "workspace_root": "/workspace",
                "active_file": {
                    "path": "/workspace/script.py",
                    "language_id": "python",
                    "dirty": false,
                    "truncated": false
                }
            }"#,
        )
        .expect("write snapshot");

        unsafe {
            env::set_var(IDE_CONTEXT_ENV_VAR, &path);
        }

        assert_eq!(
            preferred_display_language_for_workspace(workspace.path()),
            Some("Python".to_string())
        );

        unsafe {
            env::remove_var(IDE_CONTEXT_ENV_VAR);
        }
    }

    #[test]
    #[serial]
    fn preferred_language_reads_workspace_snapshot_without_env() {
        let workspace = TempDir::new().expect("workspace tempdir");
        let snapshot_dir = workspace.path().join(".vtcode");
        fs::create_dir_all(&snapshot_dir).expect("create snapshot dir");
        fs::write(
            snapshot_dir.join("ide-context.json"),
            r#"{
                "version": 1,
                "provider_family": "generic",
                "workspace_root": "/workspace",
                "active_file": {
                    "path": "/workspace/script.py",
                    "language_id": "python",
                    "dirty": false,
                    "truncated": false
                }
            }"#,
        )
        .expect("write snapshot");

        unsafe {
            env::remove_var(IDE_CONTEXT_ENV_VAR);
        }

        assert_eq!(
            preferred_display_language_for_workspace(workspace.path()),
            Some("Python".to_string())
        );
    }
}
