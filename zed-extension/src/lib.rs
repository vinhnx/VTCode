use std::sync::Arc;
use zed_extension_api as zed;

mod cache;
mod command_builder;
mod commands;
mod config;
mod context;
mod editor;
mod error_handling;
mod executor;
mod metrics;
mod output;
mod validation;
mod workspace;

pub use cache::{
    CacheEntry, CacheStats, CommandResultCache, FileContentCache, WorkspaceAnalysisCache,
    WorkspaceAnalysisData,
};
pub use command_builder::CommandBuilder;
pub use commands::{
    analyze_workspace, ask_about_selection, ask_agent, check_status, launch_chat, CommandResponse,
};
pub use config::{find_config, load_config, Config};
pub use context::{Diagnostic, DiagnosticSeverity, EditorContext, QuickFix};
pub use editor::{EditorState, StatusIndicator};
pub use error_handling::{
    ErrorCode, ErrorSeverity, ExtensionError, ExtensionResult, RecoveryStrategy,
};
pub use executor::{
    check_vtcode_available, execute_command, execute_command_with_timeout, get_vtcode_version,
};
pub use metrics::{CommandTimer, MetricStats, MetricsCollector};
pub use output::{MessageType, OutputChannel, OutputMessage};
pub use validation::{validate_config, ValidationError, ValidationResult};
pub use workspace::{
    DirectoryNode, FileContentContext, OpenBuffer, OpenBuffersContext, ProjectStructure,
    WorkspaceContext, WorkspaceFile,
};

/// VTCode extension for Zed editor
/// Provides integration with the VTCode AI coding assistant
pub struct VTCodeExtension {
    config: Option<Config>,
    vtcode_available: bool,
    output_channel: Arc<OutputChannel>,
    editor_state: Arc<EditorState>,
}

impl zed::Extension for VTCodeExtension {
    fn new() -> Self {
        let vtcode_available = check_vtcode_available();
        let extension = Self {
            config: None,
            vtcode_available,
            output_channel: Arc::new(OutputChannel::new()),
            editor_state: Arc::new(EditorState::new()),
        };

        // Update status based on CLI availability
        if vtcode_available {
            let _ = extension.editor_state.set_status(StatusIndicator::Ready);
        }

        extension
    }
}

impl VTCodeExtension {
    /// Initialize the extension with workspace configuration
    pub fn initialize(&mut self, workspace_root: &str) -> Result<(), String> {
        // Try to load configuration from workspace
        let path = std::path::Path::new(workspace_root);
        if let Some(config) = find_config(path) {
            self.config = Some(config);
        }

        // Verify VTCode CLI is available
        if !self.vtcode_available {
            return Err("VTCode CLI not found in PATH".to_string());
        }

        Ok(())
    }

    /// Get the current configuration
    pub fn config(&self) -> Option<&Config> {
        self.config.as_ref()
    }

    /// Check if VTCode CLI is available
    pub fn is_vtcode_available(&self) -> bool {
        self.vtcode_available
    }

    /// Execute "Ask the Agent" command
    pub fn ask_agent_command(&self, query: &str) -> CommandResponse {
        ask_agent(query)
    }

    /// Execute "Ask About Selection" command
    pub fn ask_about_selection_command(
        &self,
        code: &str,
        language: Option<&str>,
    ) -> CommandResponse {
        ask_about_selection(code, language)
    }

    /// Execute "Analyze Workspace" command
    pub fn analyze_workspace_command(&self) -> CommandResponse {
        analyze_workspace()
    }

    /// Execute "Launch Chat" command
    pub fn launch_chat_command(&self) -> CommandResponse {
        launch_chat()
    }

    /// Execute "Check Status" command
    pub fn check_status_command(&self) -> CommandResponse {
        check_status()
    }

    /// Get the output channel
    pub fn output_channel(&self) -> Arc<OutputChannel> {
        Arc::clone(&self.output_channel)
    }

    /// Log a command execution to the output channel
    pub fn log_command_execution(&self, command: &str, response: &CommandResponse) {
        if response.success {
            self.output_channel.success(format!(
                "Command '{}' completed:\n{}",
                command, response.output
            ));
        } else {
            self.output_channel.error(format!(
                "Command '{}' failed: {}",
                command,
                response
                    .error
                    .as_ref()
                    .unwrap_or(&"Unknown error".to_string())
            ));
        }
    }

    /// Get the editor state
    pub fn editor_state(&self) -> Arc<EditorState> {
        Arc::clone(&self.editor_state)
    }

    /// Update editor context from current selection
    pub fn update_editor_context(&self, context: EditorContext) {
        let _ = self.editor_state.set_context(context);
    }

    /// Execute a command and update status
    pub fn execute_with_status(&self, command: &str, query: &str) -> CommandResponse {
        // Set executing status
        let _ = self.editor_state.set_status(StatusIndicator::Executing);
        self.output_channel.info(format!("Executing: {}", command));

        // Execute command
        let response = ask_agent(query);

        // Update status based on result
        if response.success {
            let _ = self.editor_state.set_status(StatusIndicator::Ready);
            self.output_channel
                .success(format!("Command '{}' completed successfully", command));
        } else {
            let _ = self.editor_state.set_status(StatusIndicator::Error);
            self.output_channel.error(format!(
                "Command '{}' failed: {}",
                command,
                response
                    .error
                    .as_ref()
                    .unwrap_or(&"Unknown error".to_string())
            ));
        }

        response
    }

    /// Add an inline diagnostic
    pub fn add_diagnostic(&self, diagnostic: Diagnostic) {
        let _ = self.editor_state.add_diagnostic(diagnostic);
    }

    /// Clear all diagnostics
    pub fn clear_diagnostics(&self) {
        let _ = self.editor_state.clear_diagnostics();
    }

    /// Add a quick fix suggestion
    pub fn add_quick_fix(&self, fix: QuickFix) {
        let _ = self.editor_state.add_quick_fix(fix);
    }

    /// Get diagnostic summary for status bar
    pub fn diagnostic_summary(&self) -> String {
        self.editor_state
            .diagnostic_summary()
            .unwrap_or_else(|_| "Error retrieving diagnostics".to_string())
    }

    /// Validate the current configuration
    pub fn validate_current_config(&self) -> ValidationResult {
        match &self.config {
            Some(config) => validate_config(config),
            None => ValidationResult::ok()
                .with_warning("No configuration file found, using defaults".to_string()),
        }
    }

    /// Log configuration validation results
    pub fn log_validation(&self, result: &ValidationResult) {
        if result.valid {
            self.output_channel.success(format!(
                "Configuration validation passed\n{}",
                result.format()
            ));
        } else {
            self.output_channel.error(format!(
                "Configuration validation failed\n{}",
                result.format()
            ));
        }
    }
}

zed::register_extension!(VTCodeExtension);

#[cfg(test)]
mod tests {
    use super::*;
    use zed_extension_api::Extension;

    #[test]
    fn test_extension_creation() {
        let ext = VTCodeExtension::new();
        assert!(ext.config.is_none());
        // vtcode_available depends on system, don't assert
    }

    #[test]
    fn test_config_getter() {
        let ext = VTCodeExtension::new();
        assert!(ext.config().is_none());
    }

    #[test]
    fn test_vtcode_availability_check() {
        let ext = VTCodeExtension::new();
        // Just verify the method exists and returns a bool
        let _ = ext.is_vtcode_available();
    }
}
