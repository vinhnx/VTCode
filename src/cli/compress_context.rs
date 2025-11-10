use anyhow::Result;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

// Removed: Context compression command has been removed as part of complete context optimization feature removal
pub async fn handle_compress_context_command(_config: &CoreAgentConfig) -> Result<()> {
    anyhow::bail!("Context compression command has been removed as part of context optimization feature removal")
}
