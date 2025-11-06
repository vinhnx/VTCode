use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::warn;
use vtcode_core::sandbox::{
    DomainAddition, DomainRemoval, PathAddition, PathRemoval, SandboxEnvironment, SandboxProfile,
    SandboxRuntimeKind,
};
use vtcode_core::tools::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use which::which;

use super::slash_commands::SandboxAction;

const SANDBOX_RUNTIME_ENV: &str = "VT_SANDBOX_RUNTIME";
const SRT_PATH_ENV: &str = "SRT_PATH";
const FIRECRACKER_PATH_ENV: &str = "FIRECRACKER_PATH";
const FIRECRACKER_LAUNCHER_ENV: &str = "FIRECRACKER_LAUNCHER_PATH";
const EVENT_LOG_FILENAME: &str = "events.log";
const PERSISTENT_DIR_NAME: &str = "persistent";

/// Coordinates runtime sandbox configuration for the Bash tool.
pub(crate) struct SandboxCoordinator {
    environment: SandboxEnvironment,
    profile: Option<SandboxProfile>,
    runtime_path: Option<PathBuf>,
}

impl SandboxCoordinator {
    pub(crate) fn new(workspace_root: PathBuf) -> Self {
        let resolved_workspace = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.clone());
        let sandbox_root = resolved_workspace.join(".vtcode").join("sandbox");
        let runtime_kind = detect_runtime_kind();
        let environment = SandboxEnvironment::builder(resolved_workspace)
            .sandbox_root(sandbox_root)
            .persistent_dir_name(PERSISTENT_DIR_NAME)
            .event_log_filename(EVENT_LOG_FILENAME)
            .settings_filename("settings.json")
            .runtime_kind(runtime_kind)
            .build();

        Self {
            environment,
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
                self.refresh_profile(registry);
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
                self.refresh_profile(registry);
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
                self.refresh_profile(registry);
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
                self.refresh_profile(registry);
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

    fn enable(&mut self, registry: &mut ToolRegistry, renderer: &mut AnsiRenderer) -> Result<()> {
        self.sync_settings()?;
        let binary_path = self.resolve_runtime()?;
        let profile = self.environment.create_profile(binary_path.clone());
        registry
            .pty_manager()
            .set_sandbox_profile(Some(profile.clone()));
        self.profile = Some(profile);
        self.runtime_path = Some(binary_path.clone());
        renderer.line(
            MessageStyle::Info,
            "Sandboxing enabled for bash tool. Network access now requires /sandbox allow-domain <domain>.",
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Sandbox settings: {}",
                self.environment.settings_path().display()
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Sandbox runtime ({}): {}",
                self.environment.runtime_kind(),
                binary_path.display()
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Persistent storage: {}",
                self.environment.persistent_storage().display()
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Sandbox event log: {}",
                self.environment.event_log_path().display()
            ),
        )?;
        if let Err(error) = self.environment.log_event("Sandbox enabled for bash tool") {
            warn!("failed to record sandbox enablement: {error}");
        }
        Ok(())
    }

    fn disable(&mut self, registry: &mut ToolRegistry, renderer: &mut AnsiRenderer) -> Result<()> {
        self.profile = None;
        self.runtime_path = None;
        registry.pty_manager().set_sandbox_profile(None);

        renderer.line(MessageStyle::Info, "Sandboxing disabled for bash tool.")?;
        if let Err(error) = self.environment.log_event("Sandbox disabled for bash tool") {
            warn!("failed to record sandbox disablement: {error}");
        }
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.profile.is_some()
    }

    fn resolve_runtime(&self) -> Result<PathBuf> {
        match self.environment.runtime_kind() {
            SandboxRuntimeKind::AnthropicSrt => {
                if let Some(path) = std::env::var_os(SRT_PATH_ENV) {
                    return Ok(PathBuf::from(path));
                }
                which("srt").context(
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
        match self.environment.allow_domain(domain)? {
            DomainAddition::Added(normalized) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!("Added '{}' to sandbox network allowlist.", normalized),
                )?;
                if let Err(error) = self.environment.log_event(&format!(
                    "Added domain '{}' to sandbox network allowlist",
                    normalized
                )) {
                    warn!("failed to record sandbox domain addition: {error}");
                }
            }
            DomainAddition::AlreadyPresent(normalized) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!("Domain '{}' is already permitted.", normalized),
                )?;
            }
        }
        Ok(())
    }

    fn remove_domain(&mut self, domain: &str, renderer: &mut AnsiRenderer) -> Result<()> {
        match self.environment.remove_domain(domain)? {
            DomainRemoval::Removed(normalized) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!("Removed '{}' from sandbox network allowlist.", normalized),
                )?;
                if let Err(error) = self.environment.log_event(&format!(
                    "Removed domain '{}' from sandbox network allowlist",
                    normalized
                )) {
                    warn!("failed to record sandbox domain removal: {error}");
                }
            }
            DomainRemoval::NotPresent(normalized) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!("Domain '{}' was not present in the allowlist.", normalized),
                )?;
            }
        }
        Ok(())
    }

    fn add_path(&mut self, path: &str, renderer: &mut AnsiRenderer) -> Result<()> {
        match self.environment.allow_path(path)? {
            PathAddition::Added(normalized) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Added '{}' to sandbox filesystem allowlist.",
                        normalized.display()
                    ),
                )?;
                if let Err(error) = self.environment.log_event(&format!(
                    "Added path '{}' to sandbox filesystem allowlist",
                    normalized.display()
                )) {
                    warn!("failed to record sandbox path addition: {error}");
                }
            }
            PathAddition::AlreadyPresent(normalized) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!("Path '{}' is already permitted.", normalized.display()),
                )?;
            }
        }
        Ok(())
    }

    fn remove_path(&mut self, path: &str, renderer: &mut AnsiRenderer) -> Result<()> {
        match self.environment.remove_path(path)? {
            PathRemoval::Removed(normalized) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Removed '{}' from sandbox filesystem allowlist.",
                        normalized.display()
                    ),
                )?;
                if let Err(error) = self.environment.log_event(&format!(
                    "Removed path '{}' from sandbox filesystem allowlist",
                    normalized.display()
                )) {
                    warn!("failed to record sandbox path removal: {error}");
                }
            }
            PathRemoval::NotPresent(normalized) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Path '{}' was not present in the filesystem allowlist.",
                        normalized.display()
                    ),
                )?;
            }
            PathRemoval::Protected(normalized) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Path '{}' is required for sandbox operation and cannot be removed.",
                        normalized.display()
                    ),
                )?;
            }
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
            &format!(
                "Settings file: {}",
                self.environment.settings_path().display()
            ),
        )?;
        if let Some(path) = &self.runtime_path {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Runtime binary ({}): {}",
                    self.environment.runtime_kind(),
                    path.display()
                ),
            )?;
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Runtime binary: pending detection (preferred runtime: {})",
                    self.environment.runtime_kind()
                ),
            )?;
        }
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Persistent storage: {}",
                self.environment.persistent_storage().display()
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!("Event log: {}", self.environment.event_log_path().display()),
        )?;
        let domains: Vec<_> = self.environment.allowed_domains().cloned().collect();
        if domains.is_empty() {
            renderer.line(
                MessageStyle::Info,
                "Network allowlist: none (all outbound requests blocked)",
            )?;
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!("Network allowlist: {}", domains.join(", ")),
            )?;
        }
        let paths: Vec<_> = self
            .environment
            .allowed_paths()
            .map(|path| path.display().to_string())
            .collect();
        if paths.is_empty() {
            renderer.line(
                MessageStyle::Info,
                "Filesystem allowlist: none (no filesystem access granted)",
            )?;
        } else {
            renderer.line(MessageStyle::Info, "Filesystem allowlist:")?;
            for path in paths {
                renderer.line(MessageStyle::Info, &format!("  - {}", path))?;
            }
        }
        let deny_rules: Vec<_> = self.environment.deny_rules().cloned().collect();
        renderer.line(
            MessageStyle::Info,
            &format!("Default read restrictions: {}", deny_rules.join(", ")),
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
        self.environment.write_settings()?;
        self.environment.ensure_persistent_storage()?;
        Ok(())
    }

    fn render_paths(&self, renderer: &mut AnsiRenderer) -> Result<()> {
        let paths: Vec<_> = self
            .environment
            .allowed_paths()
            .map(|path| path.display().to_string())
            .collect();
        if paths.is_empty() {
            renderer.line(
                MessageStyle::Info,
                "No filesystem paths are currently whitelisted for sandbox access.",
            )?;
        } else {
            renderer.line(MessageStyle::Info, "Sandbox filesystem allowlist:")?;
            for path in paths {
                renderer.line(MessageStyle::Info, &format!("  - {}", path))?;
            }
        }
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Workspace root: {}",
                self.environment.workspace_root().display()
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Persistent storage: {}",
                self.environment.persistent_storage().display()
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            "Use /sandbox allow-path <path> or /sandbox remove-path <path> to adjust access.",
        )?;
        Ok(())
    }

    fn refresh_profile(&mut self, registry: &ToolRegistry) {
        if let Some(runtime) = &self.runtime_path {
            let profile = self.environment.create_profile(runtime.clone());
            registry
                .pty_manager()
                .set_sandbox_profile(Some(profile.clone()));
            self.profile = Some(profile);
        }
    }
}

fn detect_runtime_kind() -> SandboxRuntimeKind {
    std::env::var(SANDBOX_RUNTIME_ENV)
        .ok()
        .and_then(|value| SandboxRuntimeKind::from_identifier(&value))
        .unwrap_or(SandboxRuntimeKind::AnthropicSrt)
}
