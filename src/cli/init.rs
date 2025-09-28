use crate::cli::handle_chat_command;
use anyhow::{Context, Result};
use console::style;
use std::fs;
use std::path::Path;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::{
    AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
};
use vtcode_core::ui::theme::DEFAULT_THEME_ID;

/// Handle the init command
pub async fn handle_init_command(workspace: &Path, force: bool, run: bool) -> Result<()> {
    println!("{}", style("Initialize VTCode configuration").blue().bold());
    println!("Workspace: {}", workspace.display());
    println!("Force overwrite: {}", force);
    println!("Run after init: {}", run);

    super::set_workspace_env(workspace);

    fs::create_dir_all(workspace).with_context(|| {
        format!(
            "failed to create workspace directory {}",
            workspace.display()
        )
    })?;

    // Bootstrap configuration files in the workspace
    VTCodeConfig::bootstrap_project(workspace, force)
        .with_context(|| "failed to initialize configuration files")?;

    if run {
        // After successful initialization, launch a chat session using default config
        let config = CoreAgentConfig {
            model: String::new(),
            api_key: String::new(),
            provider: String::new(),
            workspace: workspace.to_path_buf(),
            verbose: false,
            theme: DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
        };
        handle_chat_command(&config, false, false)
            .await
            .with_context(|| "failed to start chat session")?;
    }

    Ok(())
}
