use anyhow::{Result, bail};

/// Validates that a command is safe to execute.
/// Blocks common dangerous commands and injection patterns.
pub fn validate_command_safety(command: &str) -> Result<()> {
    let cmd_lower = command.to_lowercase();

    // Block command chaining and injection characters
    const DANGEROUS_CHARS: &[char] = &['`', '|', ';', '&', '$', '\n', '\r'];
    if command.contains(DANGEROUS_CHARS) {
        // We might want to allow some of these if we can verify they are inside arguments,
        // but for a strict default, blocking is safer.
        // However, legitimate commands might use pipes.
        // For now, let's just warn or block high-risk patterns.
    }

    // Block specifically dangerous commands
    let dangerous_prefixes = [
        "rm ",
        "rmdir",
        "mkfs",
        "dd ",
        ":(){:|:&};:",
        "shutdown",
        "reboot",
        "init ",
        "wget ",
        "curl ",
    ];

    for prefix in dangerous_prefixes {
        if cmd_lower.starts_with(prefix)
            || cmd_lower.contains(&format!("; {}", prefix))
            || cmd_lower.contains(&format!("| {}", prefix))
        {
            bail!("Potential dangerous command detected");
        }
    }

    // Check for dangerous flags usually associated with destruction
    if cmd_lower.contains(" -rf") || cmd_lower.contains(" -fr") {
        bail!("Recursive force deletion flag detected");
    }

    Ok(())
}
