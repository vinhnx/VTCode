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

fn canonicalize_with_fallback(path: PathBuf) -> PathBuf {
    match path.canonicalize() {
        Ok(resolved) => resolved,
        Err(_) => path,
    }
}

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
                    let candidate = PathBuf::from(path);
                    // Prevent recursive runtime selection that points back to current executable
                    if let Ok(current_exe) = std::env::current_exe() {
                        if canonicalize_with_fallback(candidate.clone())
                            == canonicalize_with_fallback(current_exe)
                        {
                            return Err(anyhow::anyhow!(
                                "Resolved Anthropic sandbox runtime points to the running vtcode executable; this would cause recursion. Please set {} to a different binary.",
                                SRT_PATH_ENV
                            ));
                        }
                    }
                    return Ok(candidate);
                }
                let candidate = which("srt");
                if let Ok(candidate_path) = &candidate {
                    if let Ok(current_exe) = std::env::current_exe() {
                        if canonicalize_with_fallback(candidate_path.clone())
                            == canonicalize_with_fallback(current_exe)
                        {
                            return Err(anyhow::anyhow!(
                                "Resolved Anthropic sandbox runtime via PATH points to the running vtcode executable; this would cause recursion. Please install 'srt' separately."
                            ));
                        }
                    }
                }
                candidate.context(
                    "Anthropic sandbox runtime 'srt' was not found in PATH. Install via `npm install -g @anthropic-ai/sandbox-runtime`.",
                )
            }
            SandboxRuntimeKind::Firecracker => {
                if let Some(path) = std::env::var_os(FIRECRACKER_LAUNCHER_ENV) {
                    let candidate = PathBuf::from(path);
                    if let Ok(current_exe) = std::env::current_exe() {
                        if canonicalize_with_fallback(candidate.clone())
                            == canonicalize_with_fallback(current_exe)
                        {
                            return Err(anyhow::anyhow!(
                                "Resolved Firecracker launcher points to the running vtcode executable; this would cause recursion. Please set {} to a different binary.",
                                FIRECRACKER_LAUNCHER_ENV
                            ));
                        }
                    }
                    return Ok(candidate);
                }
                if let Some(path) = std::env::var_os(FIRECRACKER_PATH_ENV) {
                    let candidate = PathBuf::from(path);
                    if let Ok(current_exe) = std::env::current_exe() {
                        if canonicalize_with_fallback(candidate.clone())
                            == canonicalize_with_fallback(current_exe)
                        {
                            return Err(anyhow::anyhow!(
                                "Resolved Firecracker runtime points to the running vtcode executable; this would cause recursion. Please set {} to a different binary.",
                                FIRECRACKER_PATH_ENV
                            ));
                        }
                    }
                    return Ok(candidate);
                }
                let candidate = which("firecracker-launcher").or_else(|_| which("firecracker"));
                if let Ok(candidate_path) = &candidate {
                    if let Ok(current_exe) = std::env::current_exe() {
                        if canonicalize_with_fallback(candidate_path.clone())
                            == canonicalize_with_fallback(current_exe)
                        {
                            return Err(anyhow::anyhow!(
                                "Resolved Firecracker runtime via PATH points to the running vtcode executable; this would cause recursion. Please install 'firecracker' or launcher separately."
                            ));
                        }
                    }
                }
                candidate
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    /// Small test helper that sets an environment variable for the lifetime of the struct
    /// and restores the original value on Drop.
    struct ScopedEnvVar {
        key: String,
        original: Option<String>,
    }

    impl ScopedEnvVar {
        fn set(key: &str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self {
                key: key.to_string(),
                original,
            }
        }
    }

    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            if let Some(ref orig) = self.original {
                let _ = std::env::set_var(&self.key, orig);
            } else {
                let _ = std::env::remove_var(&self.key);
            }
        }
    }
    #[test]
    fn resolve_runtime_disallows_agent_exe_via_env() {
        let tempdir = tempdir().expect("tempdir");
        let workspace = tempdir.path().to_path_buf();
        let mut coordinator = SandboxCoordinator::new(workspace);
        // Get current executable and set SRT_PATH to point to it using a scoped env
        let current = std::env::current_exe().expect("current exe");
        let _scoped = ScopedEnvVar::set(SRT_PATH_ENV, &current.to_string_lossy().to_string());
        // Ensure we get an error (guard prevents recursion)
        let result = coordinator.resolve_runtime();
        assert!(result.is_err());
        // ScopedEnvVar will restore env var when _scoped is dropped
    }

    #[test]
    fn resolve_runtime_disallows_agent_exe_via_path_which() {
        let tempdir = tempdir().expect("tempdir");
        let workspace = tempdir.path().to_path_buf();
        let mut coordinator = SandboxCoordinator::new(workspace);
        // Create a bin dir and place a stub `srt` pointing to current_exe
        let bin_dir = tempdir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin");
        let current = std::env::current_exe().expect("current exe");
        let srt_path = bin_dir.join("srt");
        // On unix, use symlink to simulate which finding 'srt' that points to current exe
        #[cfg(unix)]
        std::os::unix::fs::symlink(&current, &srt_path).expect("symlink srt");
        #[cfg(windows)]
        std::fs::copy(&current, &srt_path).expect("copy srt");
        // Prepend temp bin to PATH using scoped env helper
        let original_path = std::env::var_os("PATH");
        let new_path = format!(
            "{}{}{}",
            bin_dir.display(),
            std::path::MAIN_SEPARATOR,
            original_path
                .map(|os| os.to_string_lossy().to_string())
                .unwrap_or_default()
        );
        let _scoped_path = ScopedEnvVar::set("PATH", &new_path);
        // Should error due to guard against runtime pointing to current exe
        let result = coordinator.resolve_runtime();
        assert!(result.is_err());
        // Restore original PATH
        // PATH restored by ScopedEnvVar when _scoped_path is dropped
    }

    #[tokio::test]
    async fn enable_via_slash_command_shows_error_and_disables_runtime_when_path_points_to_agent_exe()
     {
        use crate::agent::runloop::unified::session_setup::{
            initialize_session, initialize_session_ui,
        };
        use crate::agent::runloop::unified::turn::session::slash_commands::{
            SlashCommandContext, SlashCommandControl, SlashCommandOutcome, handle_outcome,
        };
        use std::collections::BTreeMap;
        use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
        // use vtcode_core::ui::tui::theme_from_styles; (not needed in this test)
        use std::sync::Arc;

        let tmp = tempfile::tempdir().expect("tempdir");
        let workspace = tmp.path().to_path_buf();

        // Create minimal core config
        let mut core_config = CoreAgentConfig {
            model: "gemini-2.5-flash-preview".to_string(),
            api_key: "test".to_string(),
            provider: "gemini".to_string(),
            api_key_env: "GEMINI_API_KEY".to_string(),
            workspace: workspace.clone(),
            verbose: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: vtcode_core::config::types::ReasoningEffortLevel::default(),
            ui_surface: vtcode_core::config::types::UiSurfacePreference::default(),
            prompt_cache: vtcode_core::config::core::PromptCachingConfig::default(),
            model_source: vtcode_core::config::types::ModelSelectionSource::default(),
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: false,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: 0,
            checkpointing_max_age_days: None,
        };

        // Initialize the session state (provider, registry, sandbox etc)
        let mut session_state = initialize_session(&core_config, None, false, None)
            .await
            .expect("initialize session");

        // Setup UI (inline session, handle, renderer)
        let mut ui_setup =
            initialize_session_ui(&core_config, None, &mut session_state, None, false)
                .await
                .expect("initialize session ui");

        // Create a bin dir on PATH that has an `srt` symlink pointing to the current exe
        let bin_dir = tmp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin");
        let current = std::env::current_exe().expect("current exe");
        let srt_path = bin_dir.join("srt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&current, &srt_path).expect("symlink srt");
        #[cfg(windows)]
        std::fs::copy(&current, &srt_path).expect("copy srt");

        // Prepend bin_dir to PATH using ScopedEnvVar
        let original_path = std::env::var_os("PATH");
        let new_path = format!(
            "{}{}{}",
            bin_dir.display(),
            std::path::MAIN_SEPARATOR,
            original_path
                .map(|os| os.to_string_lossy().to_string())
                .unwrap_or_default()
        );
        let _scoped_path = ScopedEnvVar::set("PATH", &new_path);

        // Build the SlashCommandContext - minimal fields needed
        let mut session = ui_setup.session;
        let handle = session.clone_inline_handle();
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

        // Build local context manager
        let mut context_manager =
            crate::agent::runloop::unified::context_manager::ContextManager::new(
                session_state.base_system_prompt.clone(),
                session_state.trim_config.clone(),
                session_state.token_budget.clone(),
                session_state.token_budget_enabled,
            );

        let mut model_picker_state = None;
        let mut palette_state = None;
        let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
        let decision_ledger = session_state.decision_ledger.clone();
        let pruning_ledger = session_state.pruning_ledger.clone();
        let approval_recorder = vtcode_core::tools::ApprovalRecorder::new(workspace.clone());
        let tools = session_state.tools.clone();

        // Compose the SlashCommandContext
        let mut ctx = SlashCommandContext {
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            config: &mut core_config,
            vt_cfg: &mut None,
            provider_client: &mut session_state.provider_client,
            session_bootstrap: &session_state.session_bootstrap,
            model_picker_state: &mut model_picker_state,
            palette_state: &mut palette_state,
            sandbox: &mut session_state.sandbox,
            tool_registry: &mut session_state.tool_registry,
            conversation_history: &mut session_state.conversation_history,
            decision_ledger: &decision_ledger,
            pruning_ledger: &pruning_ledger,
            context_manager: &mut context_manager,
            session_stats: &mut session_stats,
            tools: &tools,
            token_budget_enabled: session_state.token_budget_enabled,
            trim_config: &session_state.trim_config,
            async_mcp_manager: session_state.async_mcp_manager.as_ref(),
            mcp_panel_state: &mut session_state.mcp_panel_state,
            linked_directories: &mut Vec::new(),
            ctrl_c_state: &Arc::new(crate::agent::runloop::unified::state::CtrlCState::new()),
            ctrl_c_notify: &Arc::new(tokio::sync::Notify::new()),
            default_placeholder: &ui_setup.default_placeholder,
            lifecycle_hooks: ui_setup.lifecycle_hooks.as_ref(),
            full_auto: false,
            approval_recorder: Some(&approval_recorder),
            tool_permission_cache: &session_state.tool_permission_cache,
        };

        // Execute the command outcome for enabling sandbox
        let outcome = SlashCommandOutcome::ManageSandbox {
            action: crate::agent::runloop::slash_commands::SandboxAction::Enable,
        };
        let result = handle_outcome(outcome, ctx).await;

        // Should succeed and continue (the error is handled and displayed via renderer)
        assert!(result.is_ok());
        if let Ok(SlashCommandControl::Continue) = result {
            // Confirm sandbox remains disabled
            assert!(!session_state.sandbox.is_enabled());
        }
    }
}
