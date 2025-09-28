use anyhow::Result;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

mod context;
mod git;
mod mcp_events;
mod prompt;
mod slash_commands;
mod telemetry;
mod text_tools;
mod tool_output;
mod ui;
mod unified;
mod welcome;

pub async fn run_single_agent_loop(
    config: &CoreAgentConfig,
    skip_confirmations: bool,
    full_auto: bool,
) -> Result<()> {
    let cfg_manager = ConfigManager::load_from_workspace(&config.workspace).ok();
    let vt_cfg = cfg_manager.as_ref().map(|manager| manager.config());

    unified::run_single_agent_loop_unified(config, vt_cfg, skip_confirmations, full_auto).await
}

pub(crate) fn is_context_overflow_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("context length")
        || lower.contains("context window")
        || lower.contains("maximum context")
        || lower.contains("model is overloaded")
        || lower.contains("reduce the amount")
        || lower.contains("token limit")
        || lower.contains("503")
}
