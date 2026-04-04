use crate::command_safety::shell_string_might_be_dangerous;
use anyhow::{Result, bail};
use std::path::Path;

/// Validates that a command is safe to execute.
/// Blocks common dangerous commands and injection patterns.
///
/// Optimization: Uses early returns and avoids allocations for common valid commands
pub fn validate_command_safety(command: &str) -> Result<()> {
    // Optimization: Fast path - empty or very short commands are safe patterns
    if command.len() < 3 {
        return Ok(());
    }

    let segments = split_shell_segments(command)?;

    // Reuse centralized dangerous-command detection (git/rm/mkfs/dd/etc).
    if shell_string_might_be_dangerous(command) {
        bail!("Potential dangerous command detected");
    }

    for segment in segments {
        if let Some(pattern) = additional_dangerous_pattern(&segment) {
            bail!("Potential dangerous command: {pattern}");
        }
    }

    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum QuoteState {
    None,
    Single,
    Double,
}

fn split_shell_segments(command: &str) -> Result<Vec<String>> {
    let mut segments = Vec::new();
    let mut state = QuoteState::None;
    let mut escaped = false;
    let mut segment_start = 0usize;
    let mut chars = command.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        match state {
            QuoteState::Single => {
                if ch == '\'' {
                    state = QuoteState::None;
                }
            }
            QuoteState::Double => {
                if escaped {
                    escaped = false;
                    continue;
                }

                match ch {
                    '\\' => escaped = true,
                    '"' => state = QuoteState::None,
                    '`' => bail!("Command injection pattern detected"),
                    '$' if matches!(chars.peek(), Some((_, '('))) => {
                        bail!("Command injection pattern detected");
                    }
                    _ => {}
                }
            }
            QuoteState::None => {
                if escaped {
                    escaped = false;
                    continue;
                }

                match ch {
                    '\\' => escaped = true,
                    '\'' => state = QuoteState::Single,
                    '"' => state = QuoteState::Double,
                    '`' => bail!("Command injection pattern detected"),
                    '$' if matches!(chars.peek(), Some((_, '('))) => {
                        bail!("Command injection pattern detected");
                    }
                    ';' => bail!("Unquoted command chaining detected"),
                    '\n' => bail!("Command injection pattern detected"),
                    '|' | '&' => {
                        push_segment(command, segment_start, idx, &mut segments);
                        segment_start = idx + ch.len_utf8();
                        if let Some((next_idx, next_ch)) = chars.peek().copied()
                            && next_ch == ch
                        {
                            chars.next();
                            segment_start = next_idx + next_ch.len_utf8();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    push_segment(command, segment_start, command.len(), &mut segments);
    Ok(segments)
}

fn push_segment(command: &str, start: usize, end: usize, segments: &mut Vec<String>) {
    let segment = command[start..end].trim();
    if !segment.is_empty() {
        segments.push(segment.to_string());
    }
}

fn additional_dangerous_pattern(segment: &str) -> Option<&'static str> {
    let segment_lower = segment.to_ascii_lowercase();
    if segment_lower.starts_with(":(){:|:&};:") {
        return Some(":(){:|:&};:");
    }

    let tokens = shell_words::split(segment).unwrap_or_else(|_| {
        segment
            .split_whitespace()
            .map(ToString::to_string)
            .collect()
    });
    let first = tokens.first()?;
    let command_name = base_command_name(strip_wrapping_quotes(first)).to_ascii_lowercase();

    match command_name.as_str() {
        "rmdir" => Some("rmdir"),
        "wget" => Some("wget"),
        "curl" => Some("curl"),
        "chmod"
            if tokens
                .iter()
                .skip(1)
                .any(|arg| strip_wrapping_quotes(arg).starts_with("777")) =>
        {
            Some("chmod 777")
        }
        "chown"
            if tokens.iter().skip(1).any(|arg| {
                let arg = strip_wrapping_quotes(arg).to_ascii_lowercase();
                arg == "root" || arg.starts_with("root:")
            }) =>
        {
            Some("chown root")
        }
        _ => None,
    }
}

fn strip_wrapping_quotes(token: &str) -> &str {
    token
        .strip_prefix('\'')
        .and_then(|token| token.strip_suffix('\''))
        .or_else(|| {
            token
                .strip_prefix('"')
                .and_then(|token| token.strip_suffix('"'))
        })
        .unwrap_or(token)
}

fn base_command_name(command: &str) -> &str {
    Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
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

    #[test]
    fn allows_shell_escaped_literals_with_command_substitution_chars() {
        let display = shell_words::join(["printf", "%s", "$(literal)", "`backticks`"].iter());
        let result = validate_command_safety(&display);
        assert!(result.is_ok());
    }

    #[test]
    fn allows_shell_escaped_literals_with_chaining_chars() {
        let display = shell_words::join(["printf", "%s", "; curl https://example.com"].iter());
        let result = validate_command_safety(&display);
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_unquoted_command_substitution() {
        let result = validate_command_safety("echo $(whoami)");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_command_substitution_in_double_quotes() {
        let result = validate_command_safety(r#"echo "$(whoami)""#);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_unquoted_semicolon_command_chaining() {
        let result = validate_command_safety("echo ok; pwd");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_unquoted_newline_command_chaining() {
        let result = validate_command_safety("echo ok\npwd");
        assert!(result.is_err());
    }
}
