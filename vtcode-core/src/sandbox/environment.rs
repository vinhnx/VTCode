use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};

use super::{SandboxProfile, SandboxRuntimeKind, SandboxSettings};

/// Default filesystem deny rules applied to new sandbox environments.
pub const DEFAULT_DENY_RULES: &[&str] = &[
    "Read(~/.ssh)",
    "Read(/etc/ssh)",
    "Read(/root)",
    "Read(/etc/shadow)",
];

/// Builder for [`SandboxEnvironment`] instances.
#[derive(Debug, Clone)]
pub struct SandboxEnvironmentBuilder {
    workspace_root: PathBuf,
    sandbox_root: Option<PathBuf>,
    settings_filename: String,
    persistent_dir_name: String,
    event_log_filename: String,
    runtime_kind: SandboxRuntimeKind,
    deny_rules: BTreeSet<String>,
}

impl SandboxEnvironmentBuilder {
    /// Create a new builder anchored at the provided workspace root.
    pub fn new(workspace_root: impl AsRef<Path>) -> Self {
        Self {
            workspace_root: workspace_root.as_ref().to_path_buf(),
            sandbox_root: None,
            settings_filename: "settings.json".to_string(),
            persistent_dir_name: "persistent".to_string(),
            event_log_filename: "events.log".to_string(),
            runtime_kind: SandboxRuntimeKind::AnthropicSrt,
            deny_rules: DEFAULT_DENY_RULES
                .iter()
                .map(|rule| (*rule).to_string())
                .collect(),
        }
    }

    /// Override the sandbox root directory. Relative paths are resolved against the workspace root.
    pub fn sandbox_root(mut self, sandbox_root: impl AsRef<Path>) -> Self {
        self.sandbox_root = Some(sandbox_root.as_ref().to_path_buf());
        self
    }

    /// Override the runtime kind used when building the environment.
    pub fn runtime_kind(mut self, runtime_kind: SandboxRuntimeKind) -> Self {
        self.runtime_kind = runtime_kind;
        self
    }

    /// Override the settings filename (defaults to `settings.json`).
    pub fn settings_filename(mut self, filename: impl Into<String>) -> Self {
        self.settings_filename = filename.into();
        self
    }

    /// Override the persistent storage directory name (defaults to `persistent`).
    pub fn persistent_dir_name(mut self, name: impl Into<String>) -> Self {
        self.persistent_dir_name = name.into();
        self
    }

    /// Override the event log filename (defaults to `events.log`).
    pub fn event_log_filename(mut self, filename: impl Into<String>) -> Self {
        self.event_log_filename = filename.into();
        self
    }

    /// Replace the default deny rules with the provided list.
    pub fn deny_rules<I, S>(mut self, rules: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.deny_rules = rules.into_iter().map(Into::into).collect();
        self
    }

    /// Build the [`SandboxEnvironment`].
    pub fn build(self) -> SandboxEnvironment {
        let workspace = normalize_components(&canonicalize_with_fallback(self.workspace_root));
        let sandbox_root = self
            .sandbox_root
            .map(|root| {
                if root.is_absolute() {
                    normalize_components(&canonicalize_with_fallback(root))
                } else {
                    normalize_components(&canonicalize_with_fallback(workspace.join(root)))
                }
            })
            .unwrap_or_else(|| {
                normalize_components(&canonicalize_with_fallback(workspace.join(".sandbox")))
            });

        let settings_path = normalize_components(&sandbox_root.join(self.settings_filename));
        let persistent_storage = normalize_components(&sandbox_root.join(self.persistent_dir_name));
        let events_log_path = normalize_components(&sandbox_root.join(self.event_log_filename));

        let mut allowed_paths = BTreeSet::new();
        allowed_paths.insert(workspace.clone());
        allowed_paths.insert(persistent_storage.clone());

        SandboxEnvironment {
            workspace_root: workspace,
            sandbox_root,
            settings_path,
            persistent_storage,
            events_log_path,
            runtime_kind: self.runtime_kind,
            allowed_domains: BTreeSet::new(),
            allowed_paths,
            deny_rules: self.deny_rules,
        }
    }
}

/// High-level controller for sandbox configuration, settings generation, and event logging.
#[derive(Debug, Clone)]
pub struct SandboxEnvironment {
    workspace_root: PathBuf,
    sandbox_root: PathBuf,
    settings_path: PathBuf,
    persistent_storage: PathBuf,
    events_log_path: PathBuf,
    runtime_kind: SandboxRuntimeKind,
    allowed_domains: BTreeSet<String>,
    allowed_paths: BTreeSet<PathBuf>,
    deny_rules: BTreeSet<String>,
}

impl SandboxEnvironment {
    /// Start building a sandbox environment anchored at `workspace_root`.
    pub fn builder(workspace_root: impl AsRef<Path>) -> SandboxEnvironmentBuilder {
        SandboxEnvironmentBuilder::new(workspace_root)
    }

    /// Path to the workspace root that bounds allowed filesystem access.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Directory used for sandbox settings and metadata.
    pub fn sandbox_root(&self) -> &Path {
        &self.sandbox_root
    }

    /// Path to the sandbox settings JSON file.
    pub fn settings_path(&self) -> &Path {
        &self.settings_path
    }

    /// Directory that persists sandbox state between executions.
    pub fn persistent_storage(&self) -> &Path {
        &self.persistent_storage
    }

    /// Log file capturing sandbox events.
    pub fn event_log_path(&self) -> &Path {
        &self.events_log_path
    }

    /// Current runtime kind used by the sandbox environment.
    pub fn runtime_kind(&self) -> SandboxRuntimeKind {
        self.runtime_kind
    }

    /// Update the runtime kind.
    pub fn set_runtime_kind(&mut self, runtime_kind: SandboxRuntimeKind) {
        self.runtime_kind = runtime_kind;
    }

    /// Iterator over allowed network domains.
    pub fn allowed_domains(&self) -> impl Iterator<Item = &String> + '_ {
        self.allowed_domains.iter()
    }

    /// Iterator over allowed filesystem paths.
    pub fn allowed_paths(&self) -> impl Iterator<Item = &PathBuf> + '_ {
        self.allowed_paths.iter()
    }

    /// Iterator over deny rules applied to the sandbox.
    pub fn deny_rules(&self) -> impl Iterator<Item = &String> + '_ {
        self.deny_rules.iter()
    }

    /// Grant network access to `domain`.
    pub fn allow_domain(&mut self, domain: &str) -> Result<DomainAddition> {
        let normalized = domain.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(anyhow!("Domain cannot be empty."));
        }
        if normalized.chars().any(char::is_whitespace) {
            return Err(anyhow!("Domain names cannot contain whitespace."));
        }
        if self.allowed_domains.insert(normalized.clone()) {
            Ok(DomainAddition::Added(normalized))
        } else {
            Ok(DomainAddition::AlreadyPresent(normalized))
        }
    }

    /// Revoke network access for `domain`.
    pub fn remove_domain(&mut self, domain: &str) -> Result<DomainRemoval> {
        let normalized = domain.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(anyhow!("Domain cannot be empty."));
        }
        if self.allowed_domains.remove(&normalized) {
            Ok(DomainRemoval::Removed(normalized))
        } else {
            Ok(DomainRemoval::NotPresent(normalized))
        }
    }

    /// Permit the sandbox to access `path` within the workspace boundary.
    pub fn allow_path(&mut self, path: impl AsRef<Path>) -> Result<PathAddition> {
        let normalized = self.normalize_allow_path(path.as_ref())?;
        if self.allowed_paths.insert(normalized.clone()) {
            Ok(PathAddition::Added(normalized))
        } else {
            Ok(PathAddition::AlreadyPresent(normalized))
        }
    }

    /// Remove a previously allowed path.
    pub fn remove_path(&mut self, path: impl AsRef<Path>) -> Result<PathRemoval> {
        let normalized = self.normalize_allow_path(path.as_ref())?;
        if self.is_protected_path(&normalized) {
            return Ok(PathRemoval::Protected(normalized));
        }
        if self.allowed_paths.remove(&normalized) {
            Ok(PathRemoval::Removed(normalized))
        } else {
            Ok(PathRemoval::NotPresent(normalized))
        }
    }

    /// Construct a [`SandboxProfile`] using the supplied runtime binary path.
    pub fn create_profile(&self, runtime_binary: impl Into<PathBuf>) -> SandboxProfile {
        SandboxProfile::new(
            runtime_binary.into(),
            self.settings_path.clone(),
            self.persistent_storage.clone(),
            self.allowed_paths_snapshot(),
            self.runtime_kind,
        )
    }

    /// Generate the JSON settings structure describing the sandbox configuration.
    pub fn settings(&self) -> SandboxSettings {
        SandboxSettings::new(
            self.runtime_kind,
            &self.settings_path,
            &self.persistent_storage,
            self.allow_rules(),
            self.deny_rules.iter().cloned().collect(),
            self.allowed_paths_strings(),
            self.allowed_domains.iter().cloned().collect(),
        )
    }

    /// Persist the sandbox settings JSON to disk, creating parent directories as needed.
    pub fn write_settings(&self) -> Result<()> {
        if let Some(parent) = self.settings_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create sandbox configuration directory at {}",
                    parent.display()
                )
            })?;
        }
        let contents = self
            .settings()
            .to_pretty_json()
            .context("failed to serialize sandbox settings to JSON")?;
        std::fs::write(&self.settings_path, contents).with_context(|| {
            format!(
                "failed to write sandbox settings to {}",
                self.settings_path.display()
            )
        })?;
        Ok(())
    }

    /// Ensure the persistent storage directory exists.
    pub fn ensure_persistent_storage(&self) -> Result<()> {
        std::fs::create_dir_all(&self.persistent_storage).with_context(|| {
            format!(
                "failed to create sandbox persistent storage at {}",
                self.persistent_storage.display()
            )
        })
    }

    /// Append a human-readable event entry to the sandbox event log.
    pub fn log_event(&self, message: &str) -> Result<()> {
        if message.trim().is_empty() {
            return Ok(());
        }
        if let Some(parent) = self.events_log_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create sandbox event log directory at {}",
                    parent.display()
                )
            })?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.events_log_path)
            .with_context(|| {
                format!(
                    "failed to open sandbox event log at {}",
                    self.events_log_path.display()
                )
            })?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        writeln!(file, "[{timestamp}] {message}").context("failed to write sandbox event entry")?;
        Ok(())
    }

    fn allowed_paths_snapshot(&self) -> Vec<PathBuf> {
        self.allowed_paths.iter().cloned().collect()
    }

    fn allowed_paths_strings(&self) -> Vec<String> {
        self.allowed_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect()
    }

    fn allow_rules(&self) -> Vec<String> {
        let mut rules: BTreeSet<String> = BTreeSet::new();
        for path in &self.allowed_paths {
            let display = path.display();
            rules.insert(format!("Edit({display})"));
            rules.insert(format!("Read({display})"));
        }
        rules.insert("Read(.)".to_string());
        for domain in &self.allowed_domains {
            rules.insert(format!("WebFetch(domain:{})", domain));
        }
        rules.into_iter().collect()
    }

    fn normalize_allow_path(&self, raw: &Path) -> Result<PathBuf> {
        let combined = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            self.workspace_root.join(raw)
        };
        let normalized = normalize_components(&combined);
        if !normalized.starts_with(self.canonical_workspace()) {
            return Err(anyhow!(
                "Path '{}' escapes workspace '{}'",
                normalized.display(),
                self.workspace_root.display()
            ));
        }
        Ok(normalized)
    }

    fn is_protected_path(&self, candidate: &Path) -> bool {
        let normalized = normalize_components(candidate);
        normalized == self.canonical_workspace()
            || normalized == self.canonical_persistent_storage()
    }

    fn canonical_workspace(&self) -> PathBuf {
        canonicalize_with_fallback(self.workspace_root.clone())
    }

    fn canonical_persistent_storage(&self) -> PathBuf {
        canonicalize_with_fallback(self.persistent_storage.clone())
    }
}

/// Result of attempting to add a domain to the sandbox network allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainAddition {
    Added(String),
    AlreadyPresent(String),
}

/// Result of attempting to remove a domain from the sandbox network allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainRemoval {
    Removed(String),
    NotPresent(String),
}

/// Result of attempting to add a filesystem path to the sandbox allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathAddition {
    Added(PathBuf),
    AlreadyPresent(PathBuf),
}

/// Result of attempting to remove a filesystem path from the sandbox allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathRemoval {
    Removed(PathBuf),
    NotPresent(PathBuf),
    Protected(PathBuf),
}

fn canonicalize_with_fallback(path: PathBuf) -> PathBuf {
    match path.canonicalize() {
        Ok(resolved) => resolved,
        Err(_) => path,
    }
}

fn normalize_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}
