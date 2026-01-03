use anyhow::{Result, bail};

/// Validates that a command is safe to execute.
/// Blocks common dangerous commands and injection patterns.
pub fn validate_command_safety(command: &str) -> Result<()> {
    let cmd_lower = command.to_lowercase();

    // High-risk injection patterns (always blocked)
    // Command substitution and newline injection
    if command.contains('`') || command.contains("$(") || command.contains('\n') {
        bail!("Command injection pattern detected");
    }

    // Check for unquoted semicolons (command chaining)
    if let Some(pos) = command.find(';') {
        if !is_in_quotes(command, pos) {
            bail!("Unquoted command chaining detected");
        }
    }

    // Block specifically dangerous commands
    const DANGEROUS_PREFIXES: &[&str] = &[
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
        "chmod 777",
        "chown root",
    ];

    for prefix in DANGEROUS_PREFIXES {
        if cmd_lower.starts_with(prefix)
            || cmd_lower.contains(&format!("; {}", prefix))
            || cmd_lower.contains(&format!("| {}", prefix))
            || cmd_lower.contains(&format!("&& {}", prefix))
        {
            bail!("Potential dangerous command: {}", prefix.trim());
        }
    }

    // Check for dangerous flags usually associated with destruction
    if cmd_lower.contains(" -rf") || cmd_lower.contains(" -fr") {
        bail!("Recursive force deletion flag detected");
    }

    Ok(())
}

/// Check if a position in a string is inside quotes (single or double)
fn is_in_quotes(s: &str, pos: usize) -> bool {
    let before = &s[..pos];
    let single_quotes = before.matches('\'').count();
    let double_quotes = before.matches('"').count();
    (single_quotes % 2 == 1) || (double_quotes % 2 == 1)
}
