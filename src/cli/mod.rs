use std::path::PathBuf;

use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;

// Re-export the core CLI functions we need
pub use vtcode_core::mcp::cli::handle_mcp_command;

pub mod analyze;

#[allow(dead_code)]
#[derive(Default)]
pub struct AskCommandOptions {
    pub output_format: Option<vtcode_core::cli::args::AskOutputFormat>,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub skip_confirmations: bool,
}

#[allow(dead_code)]
pub struct ExecCommandOptions {
    pub json: bool,
    pub events_path: Option<PathBuf>,
    pub last_message_file: Option<PathBuf>,
}

#[allow(dead_code)]
pub struct BenchmarkCommandOptions {
    pub task_file: Option<PathBuf>,
    pub inline_task: Option<String>,
    pub output: Option<PathBuf>,
    pub max_tasks: Option<usize>,
}

#[allow(dead_code)]
pub struct SkillsCommandOptions {
    pub workspace: PathBuf,
}

// Marketplace command handlers - these are the new functions we're adding
pub async fn handle_marketplace_add(source: String, id: Option<String>) -> Result<()> {
    println!("Adding marketplace: {} with id: {:?}", source, id);

    // In a full implementation, this would:
    // 1. Parse the source to determine if it's a GitHub repo, Git URL, local path, or remote URL
    // 2. Download the marketplace manifest
    // 3. Register the marketplace in the configuration
    // 4. Cache the plugin listings

    println!("Marketplace add functionality would be implemented here");
    Ok(())
}

pub async fn handle_marketplace_list() -> Result<()> {
    println!("Listing configured marketplaces...");

    // In a full implementation, this would:
    // 1. Read the marketplace configuration
    // 2. Show all registered marketplaces with their status
    // 3. Potentially show available plugins from each marketplace

    println!("Marketplace list functionality would be implemented here");
    Ok(())
}

pub async fn handle_marketplace_remove(id: String) -> Result<()> {
    println!("Removing marketplace: {}", id);

    // In a full implementation, this would:
    // 1. Remove the marketplace from configuration
    // 2. Potentially uninstall plugins from that marketplace (with user confirmation)
    // 3. Clean up cached data

    println!("Marketplace remove functionality would be implemented here");
    Ok(())
}

pub async fn handle_marketplace_update(id: Option<String>) -> Result<()> {
    match id {
        Some(marketplace_id) => println!("Updating marketplace: {}", marketplace_id),
        None => println!("Updating all marketplaces..."),
    }

    // In a full implementation, this would:
    // 1. Fetch updated manifests from the marketplace(s)
    // 2. Update the cached plugin listings
    // 3. Potentially notify about new plugins available

    println!("Marketplace update functionality would be implemented here");
    Ok(())
}

pub async fn handle_plugin_install(name: String, marketplace: Option<String>) -> Result<()> {
    use vtcode_core::config::PluginRuntimeConfig;
    use vtcode_core::plugins::{PluginManager, PluginSource};

    // Get the plugin directory from config
    let plugins_dir = vtcode_core::config::defaults::get_data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("./.vtcode/plugins"));

    // Create a basic plugin runtime config
    let config = PluginRuntimeConfig::default();

    // Create the plugin manager
    let manager = PluginManager::new(config, plugins_dir)?;

    // Determine the source based on marketplace
    let source = if let Some(marketplace_id) = marketplace {
        PluginSource::Marketplace(format!("{}/{}", marketplace_id, &name))
    } else {
        // If no marketplace specified, assume it's a local path or Git URL
        if name.starts_with("http") || name.starts_with("git@") {
            PluginSource::Git(name.clone())
        } else if std::path::Path::new(&name).exists() {
            PluginSource::Local(std::path::PathBuf::from(&name))
        } else {
            // Assume it's a marketplace plugin without explicit marketplace
            PluginSource::Marketplace(name.clone())
        }
    };

    // Install the plugin
    manager
        .install_plugin(source, Some(name.clone()))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to install plugin {}: {}", name, e))?;

    println!("Successfully installed plugin: {}", name);
    Ok(())
}

pub async fn handle_plugin_list() -> Result<()> {
    use vtcode_core::config::PluginRuntimeConfig;
    use vtcode_core::plugins::PluginManager;

    // Get the plugin directory from config
    let plugins_dir = vtcode_core::config::defaults::get_data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("./.vtcode/plugins"));

    // Create a basic plugin runtime config
    let config = PluginRuntimeConfig::default();

    // Create the plugin manager
    let manager = PluginManager::new(config, plugins_dir)?;

    // List installed plugins
    let installed_plugins = manager
        .list_installed_plugins()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list installed plugins: {}", e))?;

    if installed_plugins.is_empty() {
        println!("No plugins installed.");
    } else {
        println!("Installed plugins:");
        for plugin in installed_plugins {
            let is_enabled = manager.is_plugin_enabled(&plugin).await;
            let status = if is_enabled { "enabled" } else { "disabled" };
            println!("  - {} ({})", plugin, status);
        }
    }
    Ok(())
}

pub async fn handle_plugin_uninstall(name: String) -> Result<()> {
    use vtcode_core::config::PluginRuntimeConfig;
    use vtcode_core::plugins::PluginManager;

    // Get the plugin directory from config
    let plugins_dir = vtcode_core::config::defaults::get_data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("./.vtcode/plugins"));

    // Create a basic plugin runtime config
    let config = PluginRuntimeConfig::default();

    // Create the plugin manager
    let manager = PluginManager::new(config, plugins_dir)?;

    // Uninstall the plugin
    manager
        .uninstall_plugin(&name)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to uninstall plugin {}: {}", name, e))?;

    println!("Successfully uninstalled plugin: {}", name);
    Ok(())
}

pub async fn handle_plugin_enable(name: String) -> Result<()> {
    use vtcode_core::config::PluginRuntimeConfig;
    use vtcode_core::plugins::PluginManager;

    // Get the plugin directory from config
    let plugins_dir = vtcode_core::config::defaults::get_data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("./.vtcode/plugins"));

    // Create a basic plugin runtime config
    let config = PluginRuntimeConfig::default();

    // Create the plugin manager
    let manager = PluginManager::new(config, plugins_dir)?;

    // Enable the plugin
    manager
        .enable_plugin(&name)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to enable plugin {}: {}", name, e))?;

    println!("Successfully enabled plugin: {}", name);
    Ok(())
}

pub async fn handle_plugin_disable(name: String) -> Result<()> {
    use vtcode_core::config::PluginRuntimeConfig;
    use vtcode_core::plugins::PluginManager;

    // Get the plugin directory from config
    let plugins_dir = vtcode_core::config::defaults::get_data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("./.vtcode/plugins"));

    // Create a basic plugin runtime config
    let config = PluginRuntimeConfig::default();

    // Create the plugin manager
    let manager = PluginManager::new(config, plugins_dir)?;

    // Disable the plugin
    manager
        .disable_plugin(&name)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to disable plugin {}: {}", name, e))?;

    println!("Successfully disabled plugin: {}", name);
    Ok(())
}

// Plugin validation command
pub async fn handle_plugin_validate(path: &std::path::Path) -> Result<()> {
    vtcode_core::plugins::handle_plugin_validate(path).await
}

// For the other functions, we'll use proper implementations that match the expected signatures
pub async fn handle_acp_command(
    _core_cfg: &vtcode_core::config::types::AgentConfig,
    _cfg: &VTCodeConfig,
    _target: vtcode_core::cli::args::AgentClientProtocolTarget,
) -> Result<()> {
    // This function should delegate to the actual implementation
    // For now, we'll just return an error to indicate it's not implemented
    Err(anyhow::anyhow!("ACP command not implemented in this stub"))
}

pub async fn handle_ask_single_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    prompt: Option<String>,
    _options: AskCommandOptions,
) -> Result<()> {
    // Import the actual implementation from the ask module
    let prompt_vec = if let Some(p) = prompt {
        vec![p]
    } else {
        vec![]
    };
    vtcode_core::commands::ask::handle_ask_command(core_cfg, prompt_vec).await
}

pub async fn handle_chat_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    skip_confirmations: bool,
    full_auto_requested: bool,
) -> Result<()> {
    crate::agent::agents::run_single_agent_loop(
        &core_cfg,
        skip_confirmations,
        full_auto_requested,
        None,
    )
    .await
}

pub async fn handle_exec_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _cfg: &VTCodeConfig,
    _options: ExecCommandOptions,
    _prompt: Option<String>,
) -> Result<()> {
    Err(anyhow::anyhow!("Exec command not implemented in this stub"))
}

pub async fn handle_analyze_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    analysis_type: analyze::AnalysisType,
) -> Result<()> {
    // Convert AnalysisType to string for the actual handler
    let depth = match analysis_type {
        analyze::AnalysisType::Full
        | analyze::AnalysisType::Structure
        | analyze::AnalysisType::Complexity => "deep",
        analyze::AnalysisType::Security
        | analyze::AnalysisType::Performance
        | analyze::AnalysisType::Dependencies => "standard",
    };

    // Use "text" as default format
    let format = "text";

    vtcode_core::commands::analyze::handle_analyze_command(
        core_cfg,
        depth.to_string(),
        format.to_string(),
    )
    .await
}

pub async fn handle_trajectory_logs_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _file: Option<PathBuf>,
    _top: Option<usize>,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Trajectory logs command not implemented in this stub"
    ))
}

pub async fn handle_create_project_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _name: &str,
    _features: &[String],
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Create project command not implemented in this stub"
    ))
}

pub async fn handle_revert_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _turn: usize,
    _partial: Option<String>,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Revert command not implemented in this stub"
    ))
}

pub async fn handle_snapshots_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Snapshots command not implemented in this stub"
    ))
}

pub async fn handle_cleanup_snapshots_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _max_snapshots: Option<usize>,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Cleanup snapshots command not implemented in this stub"
    ))
}

pub async fn handle_init_command(_workspace: &PathBuf, _force: bool, _migrate: bool) -> Result<()> {
    Err(anyhow::anyhow!("Init command not implemented in this stub"))
}

pub async fn handle_config_command(_output: Option<PathBuf>, _global: bool) -> Result<()> {
    Err(anyhow::anyhow!(
        "Config command not implemented in this stub"
    ))
}

pub async fn handle_init_project_command(
    _name: Option<String>,
    _force: bool,
    _migrate: bool,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Init project command not implemented in this stub"
    ))
}

pub async fn handle_benchmark_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _cfg: &VTCodeConfig,
    _options: BenchmarkCommandOptions,
    _full_auto_requested: bool,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Benchmark command not implemented in this stub"
    ))
}

pub async fn handle_man_command(_command: Option<String>, _output: Option<PathBuf>) -> Result<()> {
    Err(anyhow::anyhow!("Man command not implemented in this stub"))
}

pub async fn handle_resume_session_command(
    _core_cfg: &vtcode_core::config::types::AgentConfig,
    _resume_session: Option<String>,
    _custom_session_id: Option<String>,
    _skip_confirmations: bool,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Resume session command not implemented in this stub"
    ))
}

pub async fn handle_skills_list(_skills_options: &SkillsCommandOptions) -> Result<()> {
    Err(anyhow::anyhow!(
        "Skills list command not implemented in this stub"
    ))
}

pub async fn handle_skills_load(
    _skills_options: &SkillsCommandOptions,
    _name: &str,
    _path: PathBuf,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Skills load command not implemented in this stub"
    ))
}

pub async fn handle_skills_info(_skills_options: &SkillsCommandOptions, _name: &str) -> Result<()> {
    Err(anyhow::anyhow!(
        "Skills info command not implemented in this stub"
    ))
}

pub async fn handle_skills_create(_path: &PathBuf) -> Result<()> {
    Err(anyhow::anyhow!(
        "Skills create command not implemented in this stub"
    ))
}

pub async fn handle_skills_validate(_path: &PathBuf) -> Result<()> {
    Err(anyhow::anyhow!(
        "Skills validate command not implemented in this stub"
    ))
}

pub async fn handle_skills_validate_all(_skills_options: &SkillsCommandOptions) -> Result<()> {
    Err(anyhow::anyhow!(
        "Skills validate all command not implemented in this stub"
    ))
}

pub async fn handle_skills_config(_skills_options: &SkillsCommandOptions) -> Result<()> {
    Err(anyhow::anyhow!(
        "Skills config command not implemented in this stub"
    ))
}

pub async fn handle_auto_task_command(
    _core_cfg: &vtcode_core::config::types::AgentConfig,
    _cfg: &VTCodeConfig,
    _prompt: &str,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Auto task command not implemented in this stub"
    ))
}

pub fn set_workspace_env(workspace: &PathBuf) {
    unsafe {
        std::env::set_var("VTCODE_WORKSPACE", workspace);
    }
}

pub fn set_additional_dirs_env(additional_dirs: &[PathBuf]) {
    let dirs_str = additional_dirs
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(":");
    unsafe {
        std::env::set_var("VTCODE_ADDITIONAL_DIRS", dirs_str);
    }
}
