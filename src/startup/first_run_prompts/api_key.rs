use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;
use vtcode_config::write_workspace_env_value;
use vtcode_core::config::models::Provider;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

/// Prompt the user to enter an API key interactively during first-run setup.
///
/// Shows the env var name, explains where the key will be stored, and reads
/// input from stdin. The key is written to the workspace `.env` file.
/// Returns `Ok(Some(key))` if entered, `Ok(None)` if skipped.
pub(crate) fn prompt_api_key_interactive(
    renderer: &mut AnsiRenderer,
    provider: Provider,
    workspace: &Path,
) -> Result<Option<String>> {
    if provider.is_local() {
        renderer.line(
            MessageStyle::Info,
            &format!("No API key required for {} (local provider).", provider.label()),
        )?;
        return Ok(None);
    }

    let env_key = provider.default_api_key_env();
    let env_path = workspace.join(".env");

    renderer.line(
        MessageStyle::Status,
        &format!("Set up your {} API key (env: {}).", provider.label(), env_key),
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("The key will be saved to {} for this workspace.", env_path.display()),
    )?;
    renderer.line(MessageStyle::Info, "It will NOT be stored in vtcode.toml.")?;
    renderer.line(
        MessageStyle::Info,
        "Paste your API key now, or press Enter to skip (you can set it later with /model).",
    )?;

    print!("{} API key: ", provider.label());
    io::stdout()
        .flush()
        .map_err(|e| anyhow::anyhow!("Failed to flush prompt: {e}"))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| anyhow::anyhow!("Failed to read API key input: {e}"))?;

    let trimmed = input.trim();
    if trimmed.is_empty() {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Skipped. Set {env_key} in your environment or .env file before starting a chat."
            ),
        )?;
        return Ok(None);
    }

    // Basic validation: reject keys with internal whitespace (common paste mistake)
    if trimmed.chars().any(|c| c.is_whitespace()) {
        renderer.line(
            MessageStyle::Warning,
            "API key contains whitespace characters -- this is likely a paste error.",
        )?;
        renderer.line(
            MessageStyle::Info,
            "Please re-enter the key without spaces or newlines, or press Enter to skip.",
        )?;
        return Ok(None);
    }

    // Write the key to the workspace .env file
    write_workspace_env_value(workspace, env_key, trimmed)
        .map_err(|e| anyhow::anyhow!("Failed to write API key to {}: {e}", env_path.display()))?;

    renderer.line(MessageStyle::Info, &format!("API key saved to {}.", env_path.display()))?;

    Ok(Some(trimmed.to_string()))
}
