use crate::command_safety::shell_string_might_be_dangerous;
use anyhow::{Result, bail};

/// Validates that a command is safe to execute.
/// Blocks common dangerous commands and injection patterns.
///
/// Optimization: Uses early returns and avoids allocations for common valid commands
pub fn validate_command_safety(command: &str) -> Result<()> {
    // Optimization: Fast path - empty or very short commands are safe patterns
    if command.len() < 3 {
        return Ok(());
    }

    // High-risk injection patterns (always blocked)
    // Command substitution and newline injection
    // Optimization: Use byte checks for single-char patterns
    let bytes = command.as_bytes();
    if bytes.contains(&b'`') || command.contains("$(") || bytes.contains(&b'\n') {
        bail!("Command injection pattern detected");
    }

    // Check for unquoted semicolons (command chaining)
    if let Some(pos) = command.find(';')
        && !is_in_quotes(command, pos)
    {
        bail!("Unquoted command chaining detected");
    }

    // Optimization: Create lowercase only once, defer until needed
    let cmd_lower = command.to_lowercase();

    // Reuse centralized dangerous-command detection (git/rm/mkfs/dd/etc).
    if shell_string_might_be_dangerous(command) {
        bail!("Potential dangerous command detected");
    }

    // Extra command patterns not covered by the structured command safety engine.
    static ADDITIONAL_DANGEROUS_PREFIXES: &[&str] = &[
        "rmdir",
        ":(){:|:&};:",
        "wget ",
        "curl ",
        "chmod 777",
        "chown root",
    ];

    for prefix in ADDITIONAL_DANGEROUS_PREFIXES {
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

#[cfg(test)]
mod tests {
    use super::validate_command_safety;

    #[test]
    fn rejects_centrally_dangerous_command() {
        let result = validate_command_safety("git reset --hard HEAD~1");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_additional_dangerous_prefix() {
        let result = validate_command_safety("wget https://example.com/file.sh");
        assert!(result.is_err());
    }

    #[test]
    fn allows_safe_command() {
        let result = validate_command_safety("ls -la");
        assert!(result.is_ok());
    }
}
