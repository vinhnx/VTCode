//! Workspace-aware adapters that bridge `vtcode-tools` with the
//! `vtcode-commons` traits.
//!
//! These helpers allow downstream consumers to construct a `ToolRegistry`
//! using their own path, telemetry, and error-reporting implementations
//! without relying on VTCode's built-in `.vtcode` directory layout.

use std::path::{Path, PathBuf};

use anyhow::{Context, Error, Result};
use vtcode_commons::{
    ErrorFormatter, ErrorReporter, PathResolver, PathScope, TelemetrySink, WorkspacePaths,
};
use vtcode_core::config::PtyConfig;
use vtcode_core::tools::registry::ToolRegistry;

#[cfg(feature = "policies")]
use vtcode_core::tool_policy::ToolPolicyManager;

/// Telemetry events emitted by the registry builder when it resolves policy
/// storage or encounters recoverable failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryEvent {
    /// Records the resolved policy configuration file and the scope it belongs to.
    PolicyPathResolved {
        scope: PathScope,
        config_path: PathBuf,
    },
    /// Raised when telemetry recording itself fails so callers can surface the
    /// fallback behaviour.
    TelemetryFailure { message: String },
    /// Emitted when the builder reports a validation or initialization error.
    AdapterError { message: String },
}

/// Builder that wires a [`ToolRegistry`] using [`WorkspacePaths`], telemetry,
/// and error-reporting hooks supplied by the caller.
#[cfg(feature = "policies")]
pub struct RegistryBuilder<'a, Paths, Telemetry, Reporter, Formatter>
where
    Paths: WorkspacePaths + ?Sized,
    Telemetry: TelemetrySink<RegistryEvent> + ?Sized,
    Reporter: ErrorReporter + ?Sized,
    Formatter: ErrorFormatter + ?Sized,
{
    workspace_paths: &'a Paths,
    telemetry: &'a Telemetry,
    error_reporter: &'a Reporter,
    error_formatter: &'a Formatter,
    policy_manager: Option<ToolPolicyManager>,
    policy_path: Option<PathBuf>,
    pty_config: PtyConfig,
    todo_planning_enabled: bool,
}

#[cfg(feature = "policies")]
impl<'a, Paths, Telemetry, Reporter, Formatter>
    RegistryBuilder<'a, Paths, Telemetry, Reporter, Formatter>
where
    Paths: WorkspacePaths + ?Sized,
    Telemetry: TelemetrySink<RegistryEvent> + ?Sized,
    Reporter: ErrorReporter + ?Sized,
    Formatter: ErrorFormatter + ?Sized,
{
    /// Creates a new [`RegistryBuilder`] that records events via the provided
    /// telemetry sink and reports errors using the supplied reporter.
    pub fn new(
        workspace_paths: &'a Paths,
        telemetry: &'a Telemetry,
        error_reporter: &'a Reporter,
        error_formatter: &'a Formatter,
    ) -> Self {
        Self {
            workspace_paths,
            telemetry,
            error_reporter,
            error_formatter,
            policy_manager: None,
            policy_path: None,
            pty_config: PtyConfig::default(),
            todo_planning_enabled: true,
        }
    }

    /// Overrides the PTY configuration applied to the registry.
    pub fn with_pty_config(mut self, config: PtyConfig) -> Self {
        self.pty_config = config;
        self
    }

    /// Enables or disables planner-related tooling when registering built-ins.
    pub fn with_todo_planning(mut self, enabled: bool) -> Self {
        self.todo_planning_enabled = enabled;
        self
    }

    /// Supplies a pre-built [`ToolPolicyManager`], bypassing policy path resolution.
    pub fn with_policy_manager(mut self, manager: ToolPolicyManager) -> Self {
        self.policy_manager = Some(manager);
        self
    }

    /// Overrides the policy configuration path resolved from [`WorkspacePaths`].
    pub fn with_policy_path<P>(mut self, path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        self.policy_path = Some(path.into());
        self
    }

    /// Builds a [`ToolRegistry`] using the configured hooks.
    pub fn build(mut self) -> Result<ToolRegistry> {
        let workspace_root = self.workspace_paths.workspace_root().to_path_buf();
        let policy_manager = match self.policy_manager.take() {
            Some(manager) => manager,
            None => {
                let config_path = self
                    .policy_path
                    .clone()
                    .unwrap_or_else(|| self.workspace_paths.resolve_config("tool-policy.json"));
                self.record_event(RegistryEvent::PolicyPathResolved {
                    scope: self.scope_for_path(&config_path),
                    config_path: config_path.clone(),
                });

                match ToolPolicyManager::new_with_config_path(&config_path).with_context(|| {
                    format!(
                        "failed to initialize tool policy manager at {}",
                        config_path.display()
                    )
                }) {
                    Ok(manager) => manager,
                    Err(err) => {
                        self.report_error(err.clone());
                        return Err(err);
                    }
                }
            }
        };

        Ok(ToolRegistry::new_with_custom_policy_and_config(
            workspace_root,
            self.pty_config,
            self.todo_planning_enabled,
            policy_manager,
        ))
    }

    fn scope_for_path(&self, path: &Path) -> PathScope {
        if path.starts_with(self.workspace_paths.workspace_root()) {
            return PathScope::Workspace;
        }

        let config_dir = self.workspace_paths.config_dir();
        if path.starts_with(&config_dir) {
            return PathScope::Config;
        }

        if let Some(cache_dir) = self.workspace_paths.cache_dir() {
            if path.starts_with(&cache_dir) {
                return PathScope::Cache;
            }
        }

        if let Some(telemetry_dir) = self.workspace_paths.telemetry_dir() {
            if path.starts_with(&telemetry_dir) {
                return PathScope::Telemetry;
            }
        }

        PathScope::Cache
    }

    fn record_event(&self, event: RegistryEvent) {
        if let Err(err) = self.telemetry.record(&event) {
            self.handle_error(
                err.context("failed to record vtcode-tools registry adapter telemetry event"),
            );
        }
    }

    fn report_error(&self, error: Error) {
        let message = self.error_formatter.format_error(&error).into_owned();
        let _ = self.error_reporter.capture(&error);
        let _ = self
            .telemetry
            .record(&RegistryEvent::AdapterError { message });
    }

    fn handle_error(&self, error: Error) {
        let message = self.error_formatter.format_error(&error).into_owned();
        let _ = self.error_reporter.capture(&error);
        let _ = self
            .telemetry
            .record(&RegistryEvent::TelemetryFailure { message });
    }
}

#[cfg(all(test, feature = "policies"))]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use vtcode_commons::{
        DisplayErrorFormatter, MemoryErrorReporter, MemoryTelemetry, StaticWorkspacePaths,
    };

    #[test]
    fn builds_registry_with_workspace_paths() {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let config_dir = temp.path().join("config");
        std::fs::create_dir_all(&workspace_root).expect("workspace");
        std::fs::create_dir_all(&config_dir).expect("config");

        let paths = StaticWorkspacePaths::new(workspace_root.clone(), config_dir.clone());
        let telemetry = MemoryTelemetry::new();
        let reporter = MemoryErrorReporter::new();
        let formatter = DisplayErrorFormatter;

        let builder = RegistryBuilder::new(&paths, &telemetry, &reporter, &formatter);
        let registry = builder.build().expect("registry");

        assert!(registry.has_tool(vtcode_core::config::constants::tools::LIST_FILES));

        let events = telemetry.take();
        assert!(matches!(
            events.as_slice(),
            [RegistryEvent::PolicyPathResolved {
                scope: PathScope::Config,
                ..
            }]
        ));

        let policy_file = config_dir.join("tool-policy.json");
        assert!(policy_file.exists(), "policy file should be created");
        assert!(reporter.take().is_empty(), "no errors expected");
    }
}
