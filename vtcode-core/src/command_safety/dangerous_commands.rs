//! Detection of dangerous commands that should never be executed.
//!
//! This module implements hardcoded detection for commands that are inherently
//! destructive or dangerous, regardless of their options.
//!
//! Examples:
//! - `rm -rf /` (destructive)
//! - `git reset --hard` (destructive)
//! - `dd if=/dev/zero of=/dev/sda` (very destructive)
//! - `sudo rm` (privilege escalation + destruction)

/// Checks if a command appears dangerous to execute.
/// Returns true if the command should be blocked before execution.
pub fn command_might_be_dangerous(command: &[String]) -> bool {
    #[cfg(windows)]
    {
        if crate::command_safety::windows::is_dangerous_command_windows(command) {
            return true;
        }
    }

    if is_dangerous_to_call_with_exec(command) {
        return true;
    }

    // Support bash -lc "..." parsing for chained commands
    // If the command is bash -c "..." or similar, parse the script and check each command
    if command.len() >= 3
        && (command[0] == "bash" || command[0] == "sh" || command[0] == "zsh")
        && (command[1] == "-c" || command[1] == "-lc" || command[1] == "-ilc")
    {
        let script = &command[2];
        if let Ok(sub_commands) = crate::command_safety::shell_parser::parse_shell_commands(script) {
            for sub_cmd in sub_commands {
                if command_might_be_dangerous(&sub_cmd) {
                    return true;
                }
            }
        }
    }

    false
}

/// Core dangerous command detection for Unix/Linux/macOS
fn is_dangerous_to_call_with_exec(command: &[String]) -> bool {
    if command.is_empty() {
        return false;
    }

    let cmd0 = command.first().map(String::as_str);
    let base_cmd = extract_command_name(cmd0.unwrap_or(""));

    match base_cmd {
        // ──── Git ────
        "git" => {
            matches!(
                command.get(1).map(String::as_str),
                Some("reset" | "rm" | "clean")
            )
        }

        // ──── Rm ────
        "rm" => matches!(
            command.get(1).map(String::as_str),
            Some("-f" | "-rf" | "-fr" | "-r")
        ),

        // ──── Destructive system commands ────
        "mkfs" | "dd" | "shutdown" | "reboot" | "init" => true,

        // ──── Fork bomb ────
        _ if base_cmd.ends_with(':') && command.len() >= 2 => command[1] == "(){:|:&};:",

        // ──── Sudo: check the wrapped command ────
        "sudo" => {
            if command.len() > 1 {
                is_dangerous_to_call_with_exec(&command[1..])
            } else {
                false
            }
        }

        _ => false,
    }
}

/// Extract base command name from full path
fn extract_command_name(cmd: &str) -> &str {
    std::path::Path::new(cmd)
        .file_name()
        .and_then(|osstr| osstr.to_str())
        .unwrap_or(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_reset_is_dangerous() {
        let cmd = vec!["git".to_string(), "reset".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn git_reset_hard_is_dangerous() {
        let cmd = vec!["git".to_string(), "reset".to_string(), "--hard".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn git_status_is_safe() {
        let cmd = vec!["git".to_string(), "status".to_string()];
        assert!(!is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn git_log_is_safe() {
        let cmd = vec!["git".to_string(), "log".to_string()];
        assert!(!is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn rm_f_is_dangerous() {
        let cmd = vec!["rm".to_string(), "-f".to_string(), "file.txt".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn rm_rf_is_dangerous() {
        let cmd = vec!["rm".to_string(), "-rf".to_string(), "/".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn rm_without_flags_is_safe() {
        let cmd = vec!["rm".to_string()];
        assert!(!is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn mkfs_is_dangerous() {
        let cmd = vec!["mkfs".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn dd_is_dangerous() {
        let cmd = vec!["dd".to_string(), "if=/dev/zero".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn shutdown_is_dangerous() {
        let cmd = vec!["shutdown".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn sudo_git_reset_is_dangerous() {
        let cmd = vec![
            "sudo".to_string(),
            "git".to_string(),
            "reset".to_string(),
            "--hard".to_string(),
        ];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn sudo_git_status_is_safe() {
        let cmd = vec!["sudo".to_string(), "git".to_string(), "status".to_string()];
        assert!(!is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn absolute_path_git_reset_is_dangerous() {
        let cmd = vec!["/usr/bin/git".to_string(), "reset".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn empty_command_is_safe() {
        let cmd: Vec<String> = vec![];
        assert!(!is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn command_might_be_dangerous_detects_git_reset() {
        let cmd = vec!["git".to_string(), "reset".to_string()];
        assert!(command_might_be_dangerous(&cmd));
    }

    #[test]
    fn command_might_be_dangerous_allows_git_status() {
        let cmd = vec!["git".to_string(), "status".to_string()];
        assert!(!command_might_be_dangerous(&cmd));
    }
}
