use anyhow::Result;
use vtcode_core::cli::args::AgentClientProtocolTarget;

/// Detect available IDE for automatic connection when --ide flag is used.
pub(crate) fn detect_available_ide() -> Result<Option<AgentClientProtocolTarget>> {
    use std::env;

    let mut available_ides = Vec::new();

    if env::var("ZED_CLI").is_ok() || env::var("VIMRUNTIME").is_ok() {
        available_ides.push(AgentClientProtocolTarget::Zed);
    }

    match available_ides.len() {
        0 => Ok(None),
        1 => Ok(Some(available_ides[0])),
        _ => Ok(None),
    }
}
