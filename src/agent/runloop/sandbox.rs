use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use serde_json::json;
use tracing::warn;
use vtcode_core::sandbox::{SandboxProfile, SandboxRuntimeKind};
use vtcode_core::tools::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use which::which;

use super::slash_commands::SandboxAction;

const DEFAULT_DENY_RULES: &[&str] = &[
    "Read(~/.ssh)",
    "Read(/etc/ssh)",
    "Read(/root)",
    "Read(/etc/shadow)",
];

const SANDBOX_RUNTIME_ENV: &str = "VT_SANDBOX_RUNTIME";
const SRT_PATH_ENV: &str = "SRT_PATH";
const FIRECRACKER_PATH_ENV: &str = "FIRECRACKER_PATH";
const FIRECRACKER_LAUNCHER_ENV: &str = "FIRECRACKER_LAUNCHER_PATH";
const EVENT_LOG_FILENAME: &str = "events.log";
const PERSISTENT_DIR_NAME: &str = "persistent";

/// Coordinates runtime sandbox configuration for the Bash tool.
pub(crate) struct SandboxCoordinator {
    workspace_root: PathBuf,
    settings_path: PathBuf,
    allowed_domains: BTreeSet<String>,
    deny_rules: BTreeSet<String>,
    allowed_paths: BTreeSet<PathBuf>,
    persistent_storage: PathBuf,
    events_log_path: PathBuf,
    runtime_kind: SandboxRuntimeKind,
    profile: Option<SandboxProfile>,
    runtime_path: Option<PathBuf>,
}

impl SandboxCoordinator {
    pub(crate) fn new(workspace_root: PathBuf) -> Self {
        let resolved_workspace = workspace_root
            .canonicalize()
            .unwrap_or(workspace_root.clone());
        let sandbox_root = resolved_workspace
            .join(".vtcode")
            .join("sandbox")
            .canonicalize()
            .unwrap_or_else(|_| resolved_workspace.join(".vtcode").join("sandbox"));
        let settings_path = sandbox_root.join("settings.json");
        let persistent_storage = sandbox_root.join(PERSISTENT_DIR_NAME);
        let events_log_path = sandbox_root.join(EVENT_LOG_FILENAME);
        let deny_rules = DEFAULT_DENY_RULES
            .iter()
            .map(|entry| entry.to_string())
            .collect();
        let mut allowed_paths = BTreeSet::new();
        allowed_paths.insert(resolved_workspace.clone());
        allowed_paths.insert(persistent_storage.clone());
        Self {
            workspace_root: resolved_workspace,
            settings_path,
            allowed_domains: BTreeSet::new(),
            deny_rules,
            allowed_paths,
            persistent_storage,
            events_log_path,
            runtime_kind: detect_runtime_kind(),
            profile: None,
            runtime_path: None,
        }
    }

    pub(crate) fn handle_action(
        &mut self,
        action: SandboxAction,
        renderer: &mut AnsiRenderer,
        registry: &mut ToolRegistry,
    ) -> Result<()> {
        match action {
            SandboxAction::Toggle => {
                if self.is_enabled() {
                    self.disable(registry, renderer)?;
                } else {
                    self.enable(registry, renderer)?;
                }
            }
            SandboxAction::Enable => {
                if self.is_enabled() {
                    renderer.line(
                        MessageStyle::Info,
                        "Sandbox is already enabled for bash commands.",
                    )?;
                } else {
                    self.enable(registry, renderer)?;
                }
            }
            SandboxAction::Disable => {
                if self.is_enabled() {
                    self.disable(registry, renderer)?;
                } else {
                    renderer.line(MessageStyle::Info, "Sandbox is already disabled.")?;
                }
            }
            SandboxAction::Status => {
                self.render_status(renderer)?;
            }
            SandboxAction::AllowDomain(domain) => {
                self.add_domain(&domain, renderer)?;
                self.sync_settings()?;
                self.refresh_profile();
                if self.is_enabled() {
                    renderer.line(
                        MessageStyle::Info,
                        "Sandbox configuration updated; the allowlist change applies to the next command.",
                    )?;
                }
            }
            SandboxAction::RemoveDomain(domain) => {
                self.remove_domain(&domain, renderer)?;
                self.sync_settings()?;
                self.refresh_profile();
                if self.is_enabled() {
                    renderer.line(
                        MessageStyle::Info,
                        "Sandbox configuration updated; the allowlist change applies to the next command.",
                    )?;
                }
            }
            SandboxAction::AllowPath(path) => {
                self.add_path(&path, renderer)?;
                self.sync_settings()?;
                self.refresh_profile();
                if self.is_enabled() {
                    renderer.line(
                        MessageStyle::Info,
                        "Sandbox configuration updated; path allowlist change applies to the next command.",
                    )?;
                }
            }
            SandboxAction::RemovePath(path) => {
                self.remove_path(&path, renderer)?;
                self.sync_settings()?;
                self.refresh_profile();
                if self.is_enabled() {
                    renderer.line(
                        MessageStyle::Info,
                        "Sandbox configuration updated; path allowlist change applies to the next command.",
                    )?;
                }
            }
            SandboxAction::ListPaths => {
                self.render_paths(renderer)?;
            }
            SandboxAction::Help => {
                self.render_help(renderer)?;
            }
        }

        Ok(())
    }

    fn enable(&mut self, _registry: &mut ToolRegistry, renderer: &mut AnsiRenderer) -> Result<()> {
        self.sync_settings()?;
        let binary_path = self.resolve_runtime()?;
        let profile = SandboxProfile::new(
            binary_path.clone(),
            self.settings_path.clone(),
            self.persistent_storage.clone(),
            self.allowed_paths_snapshot(),
            self.runtime_kind,
        );
        self.profile = Some(profile);
        self.runtime_path = Some(binary_path.clone());
        renderer.line(
            MessageStyle::Info,
            "Sandboxing enabled for bash tool. Network access now requires /sandbox allow-domain <domain>.",
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!("Sandbox settings: {}", self.settings_path.display()),
        )?;
        if let Some(runtime) = &self.runtime_path {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Sandbox runtime ({}): {}",
                    self.runtime_kind,
                    runtime.display()
                ),
            )?;
        }
        renderer.line(
            MessageStyle::Info,
            &format!("Persistent storage: {}", self.persistent_storage.display()),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!("Sandbox event log: {}", self.events_log_path.display()),
        )?;
        if let Err(error) = self.log_event("Sandbox enabled for bash tool") {
            warn!("failed to record sandbox enablement: {error}");
        }
        Ok(())
    }

    fn disable(&mut self, _registry: &mut ToolRegistry, renderer: &mut AnsiRenderer) -> Result<()> {
        self.profile = None;

        renderer.line(MessageStyle::Info, "Sandboxing disabled for bash tool.")?;
        self.runtime_path = None;
        if let Err(error) = self.log_event("Sandbox disabled for bash tool") {
            warn!("failed to record sandbox disablement: {error}");
        }
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.profile.is_some()
    }

    fn resolve_runtime(&self) -> Result<PathBuf> {
        match self.runtime_kind {
            SandboxRuntimeKind::AnthropicSrt => {
                if let Some(path) = std::env::var_os(SRT_PATH_ENV) {
                    return Ok(PathBuf::from(path));
                }
                which("srt")
                    .context(
                        "Anthropic sandbox runtime 'srt' was not found in PATH. Install via `npm install -g @anthropic-ai/sandbox-runtime`.",
                    )
            }
            SandboxRuntimeKind::Firecracker => {
                if let Some(path) = std::env::var_os(FIRECRACKER_LAUNCHER_ENV) {
                    return Ok(PathBuf::from(path));
                }
                if let Some(path) = std::env::var_os(FIRECRACKER_PATH_ENV) {
                    return Ok(PathBuf::from(path));
                }
                which("firecracker-launcher")
                    .or_else(|_| which("firecracker"))
                    .context(
                        "Firecracker runtime was not found in PATH. Install the Firecracker launcher or set FIRECRACKER_PATH.",
                    )
            }
        }
    }

    fn add_domain(&mut self, domain: &str, renderer: &mut AnsiRenderer) -> Result<()> {
        let normalized = domain.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(anyhow!("Domain cannot be empty."));
        }
        if normalized.chars().any(char::is_whitespace) {
            return Err(anyhow!("Domain names cannot contain whitespace."));
        }
        if self.allowed_domains.insert(normalized.clone()) {
            renderer.line(
                MessageStyle::Info,
                &format!("Added '{}' to sandbox network allowlist.", normalized),
            )?;
            if let Err(error) = self.log_event(&format!(
                "Added domain '{}' to sandbox network allowlist",
                normalized
            )) {
                warn!("failed to record sandbox domain addition: {error}");
            }
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!("Domain '{}' is already permitted.", normalized),
            )?;
        }
        Ok(())
    }

    fn remove_domain(&mut self, domain: &str, renderer: &mut AnsiRenderer) -> Result<()> {
        let normalized = domain.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(anyhow!("Domain cannot be empty."));
        }
        if self.allowed_domains.remove(&normalized) {
            renderer.line(
                MessageStyle::Info,
                &format!("Removed '{}' from sandbox network allowlist.", normalized),
            )?;
            if let Err(error) = self.log_event(&format!(
                "Removed domain '{}' from sandbox network allowlist",
                normalized
            )) {
                warn!("failed to record sandbox domain removal: {error}");
            }
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!("Domain '{}' was not present in the allowlist.", normalized),
            )?;
        }
        Ok(())
    }

    fn add_path(&mut self, path: &str, renderer: &mut AnsiRenderer) -> Result<()> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("Path cannot be empty."));
        }
        let normalized = self.normalize_allow_path(trimmed)?;
        if self.allowed_paths.insert(normalized.clone()) {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Added '{}' to sandbox filesystem allowlist.",
                    normalized.display()
                ),
            )?;
            if let Err(error) = self.log_event(&format!(
                "Added path '{}' to sandbox filesystem allowlist",
                normalized.display()
            )) {
                warn!("failed to record sandbox path addition: {error}");
            }
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!("Path '{}' is already permitted.", normalized.display()),
            )?;
        }
        Ok(())
    }

    fn remove_path(&mut self, path: &str, renderer: &mut AnsiRenderer) -> Result<()> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("Path cannot be empty."));
        }
        let normalized = self.normalize_allow_path(trimmed)?;
        if self.is_protected_path(&normalized) {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Path '{}' is required for sandbox operation and cannot be removed.",
                    normalized.display()
                ),
            )?;
            return Ok(());
        }
        if self.allowed_paths.remove(&normalized) {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Removed '{}' from sandbox filesystem allowlist.",
                    normalized.display()
                ),
            )?;
            if let Err(error) = self.log_event(&format!(
                "Removed path '{}' from sandbox filesystem allowlist",
                normalized.display()
            )) {
                warn!("failed to record sandbox path removal: {error}");
            }
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Path '{}' was not present in the filesystem allowlist.",
                    normalized.display()
                ),
            )?;
        }
        Ok(())
    }

    fn render_status(&self, renderer: &mut AnsiRenderer) -> Result<()> {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Sandbox status: {}",
                if self.is_enabled() {
                    "enabled"
                } else {
                    "disabled"
                }
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!("Settings file: {}", self.settings_path.display()),
        )?;
        if let Some(path) = &self.runtime_path {
            renderer.line(
                MessageStyle::Info,
                &format!("Runtime binary ({}): {}", self.runtime_kind, path.display()),
            )?;
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Runtime binary: pending detection (preferred runtime: {})",
                    self.runtime_kind
                ),
            )?;
        }
        renderer.line(
            MessageStyle::Info,
            &format!("Persistent storage: {}", self.persistent_storage.display()),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!("Event log: {}", self.events_log_path.display()),
        )?;
        if self.allowed_domains.is_empty() {
            renderer.line(
                MessageStyle::Info,
                "Network allowlist: none (all outbound requests blocked)",
            )?;
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Network allowlist: {}",
                    self.allowed_domains
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )?;
        }
        if self.allowed_paths.is_empty() {
            renderer.line(
                MessageStyle::Info,
                "Filesystem allowlist: none (no filesystem access granted)",
            )?;
        } else {
            renderer.line(MessageStyle::Info, "Filesystem allowlist:")?;
            for path in &self.allowed_paths {
                renderer.line(MessageStyle::Info, &format!("  - {}", path.display()))?;
            }
        }
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Default read restrictions: {}",
                self.deny_rules
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            "Use /sandbox allow-domain <domain> or /sandbox remove-domain <domain> to manage network access.",
        )?;
        renderer.line(
            MessageStyle::Info,
            "Use /sandbox allow-path <path> or /sandbox remove-path <path> to manage filesystem access.",
        )?;
        Ok(())
    }

    fn render_help(&self, renderer: &mut AnsiRenderer) -> Result<()> {
        renderer.line(MessageStyle::Info, "Sandbox command usage:")?;
        renderer.line(
            MessageStyle::Info,
            "  /sandbox                 Toggle sandboxing on or off",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  /sandbox status          Show current sandbox configuration",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  /sandbox enable          Enable sandboxing explicitly",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  /sandbox disable         Disable sandboxing",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  /sandbox allow-domain <domain>   Permit outbound requests to a domain",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  /sandbox remove-domain <domain>  Revoke previously allowed domain",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  /sandbox allow-path <path>       Permit sandbox access to a workspace path",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  /sandbox remove-path <path>      Remove a previously allowed path",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  /sandbox list-paths              Show filesystem allowlist entries",
        )?;
        Ok(())
    }

    fn sync_settings(&self) -> Result<()> {
        let parent = self
            .settings_path
            .parent()
            .context("sandbox settings path missing parent directory")?
            .to_path_buf();
        let settings_path = self.settings_path.clone();
        let persistent_storage = self.persistent_storage.clone();
        let allow_rules = self.build_allow_rules();
        let deny_rules = self.build_deny_rules();
        let allowed_paths = self.allowed_paths_strings();
        let allowed_domains: Vec<_> = self.allowed_domains.iter().cloned().collect();
        let runtime_kind = self.runtime_kind;

        // Use blocking task to avoid blocking async runtime
        std::thread::spawn(move || -> Result<()> {
            std::fs::create_dir_all(&parent).with_context(|| {
                format!(
                    "failed to create sandbox configuration directory at {}",
                    parent.display()
                )
            })?;

            let config = json!({
                "sandbox": {
                    "enabled": true,
                    "runtime": runtime_kind.as_str(),
                    "settings_path": settings_path.display().to_string(),
                    "persistent_storage": persistent_storage.display().to_string(),
                },
                "permissions": {
                    "allow": allow_rules,
                    "deny": deny_rules,
                    "allowed_paths": allowed_paths,
                    "network": {
                        "allowed_domains": allowed_domains,
                    },
                },
            });
            std::fs::write(&settings_path, serde_json::to_string_pretty(&config)?).with_context(
                || {
                    format!(
                        "failed to write sandbox settings to {}",
                        settings_path.display()
                    )
                },
            )?;
            Ok(())
        })
        .join()
        .map_err(|_| anyhow::anyhow!("Sandbox sync thread panicked"))??;

        self.ensure_persistent_storage()?;
        Ok(())
    }

    fn build_allow_rules(&self) -> Vec<String> {
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

    fn build_deny_rules(&self) -> Vec<String> {
        self.deny_rules.iter().cloned().collect()
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

    fn render_paths(&self, renderer: &mut AnsiRenderer) -> Result<()> {
        if self.allowed_paths.is_empty() {
            renderer.line(
                MessageStyle::Info,
                "No filesystem paths are currently whitelisted for sandbox access.",
            )?;
        } else {
            renderer.line(MessageStyle::Info, "Sandbox filesystem allowlist:")?;
            for path in &self.allowed_paths {
                renderer.line(MessageStyle::Info, &format!("  - {}", path.display()))?;
            }
        }
        renderer.line(
            MessageStyle::Info,
            &format!("Workspace root: {}", self.workspace_root.display()),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!("Persistent storage: {}", self.persistent_storage.display()),
        )?;
        renderer.line(
            MessageStyle::Info,
            "Use /sandbox allow-path <path> or /sandbox remove-path <path> to adjust access.",
        )?;
        Ok(())
    }

    fn refresh_profile(&mut self) {
        if let Some(runtime) = &self.runtime_path {
            let profile = SandboxProfile::new(
                runtime.clone(),
                self.settings_path.clone(),
                self.persistent_storage.clone(),
                self.allowed_paths_snapshot(),
                self.runtime_kind,
            );
            self.profile = Some(profile);
        }
    }

    fn ensure_persistent_storage(&self) -> Result<()> {
        let storage = self.persistent_storage.clone();
        std::thread::spawn(move || {
            std::fs::create_dir_all(&storage).with_context(|| {
                format!(
                    "failed to create sandbox persistent storage at {}",
                    storage.display()
                )
            })
        })
        .join()
        .map_err(|_| anyhow::anyhow!("Persistent storage thread panicked"))?
    }

    fn log_event(&self, message: &str) -> Result<()> {
        if message.trim().is_empty() {
            return Ok(());
        }
        let parent = self.events_log_path.parent().map(|p| p.to_path_buf());
        let log_path = self.events_log_path.clone();
        let msg = message.to_string();

        std::thread::spawn(move || -> Result<()> {
            if let Some(parent) = parent {
                std::fs::create_dir_all(&parent).with_context(|| {
                    format!(
                        "failed to create sandbox event log directory at {}",
                        parent.display()
                    )
                })?;
            }
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .with_context(|| {
                    format!("failed to open sandbox event log at {}", log_path.display())
                })?;
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            writeln!(file, "[{timestamp}] {msg}").context("failed to write sandbox event entry")?;
            Ok(())
        })
        .join()
        .map_err(|_| anyhow::anyhow!("Log event thread panicked"))??;
        Ok(())
    }

    fn normalize_allow_path(&self, raw: &str) -> Result<PathBuf> {
        let candidate = Path::new(raw);
        let combined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.workspace_root.join(candidate)
        };
        let normalized = normalize_path(&combined);
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
        let normalized = normalize_path(candidate);
        normalized == self.canonical_workspace()
            || normalized == self.canonical_persistent_storage()
    }

    fn canonical_workspace(&self) -> PathBuf {
        match self.workspace_root.canonicalize() {
            Ok(path) => path,
            Err(_) => self.workspace_root.clone(),
        }
    }

    fn canonical_persistent_storage(&self) -> PathBuf {
        match self.persistent_storage.canonicalize() {
            Ok(path) => path,
            Err(_) => normalize_path(&self.persistent_storage),
        }
    }
}

fn detect_runtime_kind() -> SandboxRuntimeKind {
    match std::env::var(SANDBOX_RUNTIME_ENV)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "firecracker" | "firecracker-microvm" | "fc" => SandboxRuntimeKind::Firecracker,
        _ => SandboxRuntimeKind::AnthropicSrt,
    }
}

fn normalize_path(path: &Path) -> PathBuf {
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
