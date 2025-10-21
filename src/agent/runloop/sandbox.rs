use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde_json::json;
use vtcode_core::sandbox::SandboxProfile;
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

/// Coordinates runtime sandbox configuration for the Bash tool.
pub(crate) struct SandboxCoordinator {
    workspace_root: PathBuf,
    settings_path: PathBuf,
    allowed_domains: BTreeSet<String>,
    deny_rules: BTreeSet<String>,
    profile: Option<SandboxProfile>,
    runtime_path: Option<PathBuf>,
}

impl SandboxCoordinator {
    pub(crate) fn new(workspace_root: PathBuf) -> Self {
        let settings_path = workspace_root
            .join(".vtcode")
            .join("sandbox")
            .join("settings.json");
        let deny_rules = DEFAULT_DENY_RULES
            .iter()
            .map(|entry| entry.to_string())
            .collect();
        Self {
            workspace_root,
            settings_path,
            allowed_domains: BTreeSet::new(),
            deny_rules,
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
                if self.is_enabled() {
                    registry.set_bash_sandbox(self.profile.clone());
                    renderer.line(
                        MessageStyle::Info,
                        "Sandbox configuration updated; the allowlist change applies to the next command.",
                    )?;
                }
            }
            SandboxAction::RemoveDomain(domain) => {
                self.remove_domain(&domain, renderer)?;
                self.sync_settings()?;
                if self.is_enabled() {
                    registry.set_bash_sandbox(self.profile.clone());
                    renderer.line(
                        MessageStyle::Info,
                        "Sandbox configuration updated; the allowlist change applies to the next command.",
                    )?;
                }
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
        let profile = SandboxProfile::new(binary_path.clone(), self.settings_path.clone());
        self.profile = Some(profile);
        self.runtime_path = Some(binary_path);
        registry.set_bash_sandbox(self.profile.clone());
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
                &format!("Sandbox runtime: {}", runtime.display()),
            )?;
        }
        Ok(())
    }

    fn disable(&mut self, registry: &mut ToolRegistry, renderer: &mut AnsiRenderer) -> Result<()> {
        self.profile = None;
        registry.set_bash_sandbox(None);
        renderer.line(MessageStyle::Info, "Sandboxing disabled for bash tool.")?;
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.profile.is_some()
    }

    fn resolve_runtime(&self) -> Result<PathBuf> {
        if let Some(path) = std::env::var_os("SRT_PATH") {
            let candidate = PathBuf::from(path);
            return Ok(candidate);
        }
        which("srt").context(
            "Anthropic sandbox runtime 'srt' was not found in PATH. Install via `npm install -g @anthropic-ai/sandbox-runtime`.",
        )
            .map(PathBuf::from)
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
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!("Domain '{}' was not present in the allowlist.", normalized),
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
                &format!("Runtime binary: {}", path.display()),
            )?;
        } else {
            renderer.line(
                MessageStyle::Info,
                "Runtime binary: pending detection (enable sandbox to resolve)",
            )?;
        }
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
        Ok(())
    }

    fn sync_settings(&self) -> Result<()> {
        let parent = self
            .settings_path
            .parent()
            .context("sandbox settings path missing parent directory")?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create sandbox configuration directory at {}",
                parent.display()
            )
        })?;
        let allow_rules = self.build_allow_rules();
        let deny_rules = self.build_deny_rules();
        let config = json!({
            "sandbox": {
                "enabled": true,
            },
            "permissions": {
                "allow": allow_rules,
                "deny": deny_rules,
            },
        });
        fs::write(&self.settings_path, serde_json::to_string_pretty(&config)?).with_context(
            || {
                format!(
                    "failed to write sandbox settings to {}",
                    self.settings_path.display()
                )
            },
        )?;
        Ok(())
    }

    fn build_allow_rules(&self) -> Vec<String> {
        let mut rules: BTreeSet<String> = BTreeSet::new();
        let workspace = self.canonical_workspace();
        rules.insert(format!("Edit({})", workspace.display()));
        rules.insert(format!("Read({})", workspace.display()));
        rules.insert("Read(.)".to_string());
        for domain in &self.allowed_domains {
            rules.insert(format!("WebFetch(domain:{})", domain));
        }
        rules.into_iter().collect()
    }

    fn build_deny_rules(&self) -> Vec<String> {
        self.deny_rules.iter().cloned().collect()
    }

    fn canonical_workspace(&self) -> PathBuf {
        match self.workspace_root.canonicalize() {
            Ok(path) => path,
            Err(_) => self.workspace_root.clone(),
        }
    }
}
