/// Terminal title management for dynamic title updates
///
/// This module provides functionality to update the terminal title
/// based on the current agent activity state.
///
/// ## Terminal Compatibility
///
/// Uses standard OSC 2 escape sequence (`\x1b]2;title\x07`) which is
/// universally supported across:
/// - iTerm2 (macOS)
/// - Kitty (cross-platform)
/// - Alacritty (cross-platform)
/// - Ghostty (cross-platform)
/// - Warp (macOS)
/// - Terminal.app (macOS)
/// - WezTerm (cross-platform)
/// - Windows Terminal (Windows 10+)
/// - Most XTerm-compatible terminals
///
/// References:
/// - XTerm control sequences: OSC 2 sets window title
/// - Crossterm SetTitle uses OSC 2 internally
/// - OSC sequences terminated with BEL (\x07) for maximum compatibility
use super::Session;

/// Standard OSC 2 sequence for setting window title
/// Format: ESC ] 2 ; title BEL
const OSC_TITLE_PREFIX: &str = "\x1b]2;";
const OSC_TITLE_SUFFIX: &str = "\x07";

/// Maximum length for terminal title to avoid truncation issues
const MAX_TITLE_LENGTH: usize = 128;

impl Session {
    /// Set the workspace root path for dynamic title generation
    pub fn set_workspace_root(&mut self, workspace_root: Option<std::path::PathBuf>) {
        self.workspace_root = workspace_root;
    }

    /// Extract a short project name from the workspace path
    fn extract_project_name(&self) -> String {
        self.workspace_root
            .as_ref()
            .and_then(|path| {
                path.file_name()
                    .or_else(|| path.parent()?.file_name())
                    .map(|name| name.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| self.app_name.clone())
    }

    /// Strip spinner characters and leading whitespace from status text
    fn strip_spinner_prefix(text: &str) -> &str {
        text.trim_start_matches(|c: char| {
            // Braille spinner frames
            c == '⠋' || c == '⠙' || c == '⠹' || c == '⠸' || c == '⠼'
                || c == '⠴' || c == '⠦' || c == '⠧' || c == '⠇' || c == '⠏'
                // Simple spinners
                || c == '-' || c == '\\' || c == '|' || c == '/' || c == '.'
        })
        .trim_start()
    }

    /// Extract action verb from status text
    /// Status format: "⠋ Running command: cargo build" or "Running tool: read_file"
    fn extract_action_from_status(&self) -> Option<String> {
        let left = self.input_status_left.as_deref()?;
        let cleaned = Self::strip_spinner_prefix(left);

        // Check for common action patterns
        if cleaned.contains("Running command:") || cleaned.contains("Running tool:") {
            return Some("Running".to_string());
        }
        if cleaned.starts_with("Running:") || cleaned.starts_with("Running ") {
            return Some("Running".to_string());
        }
        if cleaned.starts_with("Executing") {
            return Some("Executing".to_string());
        }
        if cleaned.contains("Editing") {
            return Some("Editing".to_string());
        }
        if cleaned.contains("Debugging") {
            return Some("Debugging".to_string());
        }
        if cleaned.contains("Building") {
            return Some("Building".to_string());
        }
        if cleaned.contains("Testing") {
            return Some("Testing".to_string());
        }
        if cleaned.contains("Searching") || cleaned.contains("Finding") {
            return Some("Searching".to_string());
        }
        if cleaned.contains("Creating") {
            return Some("Creating".to_string());
        }
        if cleaned.contains("Reading") || cleaned.contains("Writing") {
            return Some("Editing".to_string());
        }
        if cleaned.contains("Waiting") || cleaned.contains("Action Required") {
            return Some("Action Required".to_string());
        }
        if cleaned.contains("Thinking") || cleaned.contains("Processing") {
            return Some("Thinking".to_string());
        }
        if cleaned.contains("Checking") {
            return Some("Checking".to_string());
        }
        if cleaned.contains("Loading") {
            return Some("Loading".to_string());
        }

        None
    }

    /// Generate the dynamic terminal title based on current state
    fn generate_terminal_title(&self) -> String {
        let project_name = self.extract_project_name();

        // Check if we're in an active state
        if let Some(action) = self.extract_action_from_status() {
            // Try to extract additional context (e.g., filename or command)
            let context = self.extract_context_from_status();

            if let Some(ctx) = context {
                // Sanitize context for terminal title (remove special chars)
                let sanitized_ctx = sanitize_for_terminal_title(&ctx);
                return truncate_title(format!(
                    "> {} ({}) | {} {}",
                    self.app_name, project_name, action, sanitized_ctx
                ));
            } else {
                return truncate_title(format!(
                    "> {} ({}) | {}",
                    self.app_name, project_name, action
                ));
            }
        }

        // Check for PTY sessions (long-running processes)
        if self.is_running_activity() {
            return truncate_title(format!("> {} ({}) | Running", self.app_name, project_name));
        }

        // Check for HITL (Human in the Loop) states
        if self.has_status_spinner() {
            return truncate_title(format!(
                "> {} ({}) | Action Required",
                self.app_name, project_name
            ));
        }

        // Default idle state
        truncate_title(format!("> {} ({})", self.app_name, project_name))
    }

    /// Extract additional context from status (filename, command, etc.)
    fn extract_context_from_status(&self) -> Option<String> {
        let left = self.input_status_left.as_deref()?;
        let cleaned = Self::strip_spinner_prefix(left);

        // Try to extract command from running status
        if cleaned.contains("Running command:") {
            // Split on "Running command:" and take the part after it
            let parts: Vec<&str> = cleaned.splitn(2, "Running command:").collect();
            if parts.len() == 2 {
                let command = parts[1].split_whitespace().next()?;
                // Get basename only (remove path)
                let cmd_name = command.split('/').next_back().unwrap_or(command);
                return Some(cmd_name.to_string());
            }
        }

        // Try to extract tool name
        if cleaned.contains("Running tool:") {
            // Split on "Running tool:" and take the part after it
            let parts: Vec<&str> = cleaned.splitn(2, "Running tool:").collect();
            if parts.len() == 2 {
                let tool = parts[1].split_whitespace().next()?;
                return Some(tool.to_string());
            }
        }

        // Try to extract filename from editing status
        if cleaned.contains("Editing") {
            // Split on "Editing" and take the part after it
            let parts: Vec<&str> = cleaned.splitn(2, "Editing").collect();
            if parts.len() == 2 {
                let after = parts[1].trim();
                // Find the end of the filename (space, colon, or end of string)
                let end_pos = after
                    .find(|c: char| c == ':' || c.is_whitespace())
                    .unwrap_or(after.len());
                let filename = after[..end_pos].trim();
                if !filename.is_empty() {
                    // Just show the filename without path
                    let name = filename.split('/').next_back().unwrap_or(filename);
                    return Some(name.to_string());
                }
            }
        }

        None
    }

    /// Update the terminal title if it has changed
    /// Uses OSC 2 escape sequence for maximum terminal compatibility
    pub fn update_terminal_title(&mut self) {
        let new_title = self.generate_terminal_title();

        // Only update if the title has changed to avoid redundant operations
        if self.last_terminal_title.as_ref() != Some(&new_title) {
            // Use OSC 2 sequence directly for better cross-terminal compatibility
            let osc_sequence = format!("{}{}{}", OSC_TITLE_PREFIX, new_title, OSC_TITLE_SUFFIX);

            use std::io::Write;
            let mut stderr = std::io::stderr();
            let _ = stderr.write_all(osc_sequence.as_bytes());
            let _ = stderr.flush();

            self.last_terminal_title = Some(new_title);
        }
    }

    /// Clear terminal title (reset to default)
    pub fn clear_terminal_title(&mut self) {
        let osc_sequence = format!("{}{}", OSC_TITLE_PREFIX, OSC_TITLE_SUFFIX);

        use std::io::Write;
        let mut stderr = std::io::stderr();
        let _ = stderr.write_all(osc_sequence.as_bytes());
        let _ = stderr.flush();

        self.last_terminal_title = None;
    }
}

/// Sanitize string for use in terminal title (remove problematic characters)
fn sanitize_for_terminal_title(s: &str) -> String {
    s.chars()
        .map(|c| {
            // Replace problematic characters with safe alternatives
            match c {
                // Remove control characters
                c if c.is_control() => ' ',
                // Replace backslash with forward slash for cleaner display
                '\\' => '/',
                // Keep alphanumeric and common punctuation
                c if c.is_ascii_alphanumeric() || "_.-".contains(c) => c,
                // Replace other special chars with space
                _ => ' ',
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

/// Truncate title to maximum length while preserving readability
fn truncate_title(title: String) -> String {
    if title.len() <= MAX_TITLE_LENGTH {
        title
    } else {
        // Truncate and add ellipsis
        let truncated = &title[..MAX_TITLE_LENGTH.saturating_sub(3)];
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_spinner_prefix() {
        assert_eq!(
            Session::strip_spinner_prefix("⠋ Running command: cargo"),
            "Running command: cargo"
        );
        assert_eq!(
            Session::strip_spinner_prefix("⠙ Running tool: test"),
            "Running tool: test"
        );
        assert_eq!(Session::strip_spinner_prefix("- Building"), "Building");
        assert_eq!(Session::strip_spinner_prefix("| Checking"), "Checking");
        assert_eq!(Session::strip_spinner_prefix("  No spinner"), "No spinner");
    }

    #[test]
    fn test_sanitize_for_terminal_title() {
        assert_eq!(sanitize_for_terminal_title("cargo build"), "cargo build");
        assert_eq!(sanitize_for_terminal_title("cargo\\build"), "cargo/build");
        assert_eq!(sanitize_for_terminal_title("test$cmd"), "test cmd");
        assert_eq!(sanitize_for_terminal_title("file\tname"), "file name");
    }

    #[test]
    fn test_truncate_title() {
        assert_eq!(truncate_title("Short".to_string()), "Short");
        let long = "a".repeat(150);
        let truncated = truncate_title(long.clone());
        assert!(truncated.len() <= MAX_TITLE_LENGTH);
        assert!(truncated.ends_with("..."));
    }
}
