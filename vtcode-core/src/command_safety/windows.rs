//! Windows/PowerShell-specific dangerous command detection.
//!
//! Detects PowerShell and CMD invocations that attempt to:
//! - Launch browsers with URLs
//! - Execute code via ShellExecute
//! - Open arbitrary files/URLs
//!
//! Examples of dangerous patterns:
//! ```powershell
//! Start-Process "https://example.com"
//! [System.Diagnostics.ProcessStartInfo]::new().UseShellExecute = $true
//! explorer.exe "https://example.com"
//! ```

/// Detects dangerous Windows/PowerShell commands
pub fn is_dangerous_command_windows(command: &[String]) -> bool {
    if command.is_empty() {
        return false;
    }

    let exe = &command[0];
    let base_exe = std::path::Path::new(exe)
        .file_name()
        .and_then(|osstr| osstr.to_str())
        .unwrap_or("")
        .to_lowercase();

    // ──── PowerShell ────
    if matches!(base_exe.as_str(), "powershell" | "pwsh" | "powershell.exe" | "pwsh.exe") {
        return is_dangerous_powershell_invocation(command);
    }

    // ──── CMD ────
    if matches!(base_exe.as_str(), "cmd" | "cmd.exe") {
        return is_dangerous_cmd_invocation(command);
    }

    false
}

/// Detects dangerous PowerShell invocations
fn is_dangerous_powershell_invocation(command: &[String]) -> bool {
    if command.len() < 2 {
        return false;
    }

    let script = &command[1];

    // Simple heuristic checks (TODO: use PowerShell parser in Phase 3)
    let script_lower = script.to_lowercase();

    // ──── Patterns that launch browsers/URLs ────
    let has_start_process = script_lower.contains("start-process")
        || script_lower.contains("saps")
        || script_lower.contains("& start-process");

    let has_invoke_item = script_lower.contains("invoke-item") || script_lower.contains("ii");

    let has_shell_execute = script_lower.contains("shellexecute");

    let has_url = script.contains("http://")
        || script.contains("https://")
        || script.contains("ftp://")
        || script.contains("file://");

    if has_url && (has_start_process || has_invoke_item || has_shell_execute) {
        return true;
    }

    // ──── Browser executables with URL ────
    let browser_patterns = [
        "firefox", "chrome", "msedge", "iexplore", "opera", "brave",
    ];
    if has_url && browser_patterns.iter().any(|b| script_lower.contains(b)) {
        return true;
    }

    // ──── Explorer with URL ────
    if has_url && (script_lower.contains("explorer.exe") || script_lower.contains("explorer ")) {
        return true;
    }

    // ──── rundll32 url.dll pattern ────
    if script_lower.contains("rundll32")
        && script_lower.contains("url.dll")
        && script_lower.contains("fileprotocolhandler")
        && has_url
    {
        return true;
    }

    // ──── mshta (HTML Application runner) ────
    if script_lower.contains("mshta") && has_url {
        return true;
    }

    false
}

/// Detects dangerous CMD invocations
fn is_dangerous_cmd_invocation(command: &[String]) -> bool {
    if command.len() < 2 {
        return false;
    }

    let rest = &command[1..];
    let mut iter = rest.iter();

    while let Some(arg) = iter.next() {
        let arg_lower = arg.to_lowercase();

        // Skip flags like /S, /V:ON, etc.
        if arg_lower.starts_with('/') && arg_lower.len() <= 3 {
            continue;
        }

        // Check for 'start' subcommand
        if arg_lower == "start" {
            // Look for URL arguments after 'start'
            if iter.any(|s| {
                let s_lower = s.to_lowercase();
                (s_lower.contains("http://")
                    || s_lower.contains("https://")
                    || s_lower.contains("ftp://"))
                    && !s_lower.starts_with('/')
            }) {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_start_process_with_url_is_dangerous() {
        let cmd = vec![
            "powershell".to_string(),
            "Start-Process 'https://example.com'".to_string(),
        ];
        assert!(is_dangerous_powershell_invocation(&cmd));
    }

    #[test]
    fn powershell_without_url_is_safe() {
        let cmd = vec![
            "powershell".to_string(),
            "Write-Host 'hello'".to_string(),
        ];
        assert!(!is_dangerous_powershell_invocation(&cmd));
    }

    #[test]
    fn powershell_invoke_item_with_url_is_dangerous() {
        let cmd = vec![
            "powershell".to_string(),
            "ii 'https://example.com'".to_string(),
        ];
        assert!(is_dangerous_powershell_invocation(&cmd));
    }

    #[test]
    fn cmd_start_with_url_is_dangerous() {
        let cmd = vec![
            "cmd".to_string(),
            "/c".to_string(),
            "start".to_string(),
            "https://example.com".to_string(),
        ];
        assert!(is_dangerous_cmd_invocation(&cmd));
    }

    #[test]
    fn cmd_start_without_url_is_safe() {
        let cmd = vec![
            "cmd".to_string(),
            "/c".to_string(),
            "start".to_string(),
            "notepad.exe".to_string(),
        ];
        assert!(!is_dangerous_cmd_invocation(&cmd));
    }

    #[test]
    fn firefox_with_url_is_dangerous() {
        let cmd = vec![
            "powershell".to_string(),
            "firefox https://example.com".to_string(),
        ];
        assert!(is_dangerous_powershell_invocation(&cmd));
    }

    #[test]
    fn explorer_with_url_is_dangerous() {
        let cmd = vec![
            "powershell".to_string(),
            "explorer.exe 'https://example.com'".to_string(),
        ];
        assert!(is_dangerous_powershell_invocation(&cmd));
    }

    #[test]
    fn is_dangerous_command_windows_detects_powershell_patterns() {
        let cmd = vec![
            "powershell".to_string(),
            "Start-Process 'https://example.com'".to_string(),
        ];
        assert!(is_dangerous_command_windows(&cmd));
    }

    #[test]
    fn is_dangerous_command_windows_detects_cmd_patterns() {
        let cmd = vec![
            "cmd.exe".to_string(),
            "/c".to_string(),
            "start".to_string(),
            "https://example.com".to_string(),
        ];
        assert!(is_dangerous_command_windows(&cmd));
    }

    #[test]
    fn empty_command_is_safe() {
        assert!(!is_dangerous_command_windows(&[]));
    }
}
