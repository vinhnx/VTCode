use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider as uni;

pub(crate) fn should_short_circuit_shell(
    input: &str,
    tool_name: &str,
    args: &serde_json::Value,
) -> bool {
    if tool_name != tools::RUN_COMMAND {
        return false;
    }

    let command = args
        .get("command")
        .and_then(|value| value.as_array())
        .and_then(|items| {
            let mut tokens = Vec::new();
            for item in items {
                if let Some(text) = item.as_str() {
                    tokens.push(text.trim_matches(|c| c == '\"' || c == '\'').to_string());
                } else {
                    return None;
                }
            }
            Some(tokens)
        });

    let Some(command_tokens) = command else {
        return false;
    };

    if command_tokens.is_empty() {
        return false;
    }

    let full_command = command_tokens.join(" ");
    if full_command.contains('|')
        || full_command.contains('>')
        || full_command.contains('<')
        || full_command.contains('&')
        || full_command.contains(';')
    {
        return false;
    }

    let user_tokens: Vec<String> = input
        .split_whitespace()
        .map(|part| part.trim_matches(|c| c == '\"' || c == '\'').to_string())
        .collect();

    if user_tokens.is_empty() {
        return false;
    }

    if user_tokens.len() != command_tokens.len() {
        return false;
    }

    user_tokens
        .iter()
        .zip(command_tokens.iter())
        .all(|(user, cmd)| user == cmd)
}

pub(crate) fn derive_recent_tool_output(history: &[uni::Message]) -> Option<String> {
    let message = history
        .iter()
        .rev()
        .find(|msg| msg.role == uni::MessageRole::Tool)?;

    let content = message.content.as_text();
    let value = serde_json::from_str::<serde_json::Value>(&content).ok()?;

    let mut output_parts = Vec::new();

    // Check for stdout
    let stdout = value
        .get("stdout")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    // Check for stderr
    let stderr = value
        .get("stderr")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    // Check for exit code
    let exit_code = value.get("exit_code").and_then(|v| v.as_i64());

    // For command execution, be more strict about missing exit codes
    // If exit_code is missing and there's no stdout/stderr, assume it was an invalid command that failed
    let has_output = stdout.is_some() || stderr.is_some();
    let success = if exit_code.is_none() && !has_output {
        // If no exit code and no output, assume failure (likely invalid command)
        false
    } else {
        exit_code.map(|code| code == 0).unwrap_or(true)
    };

    // Check for command
    let command = value
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|c| !c.is_empty());

    // Build output message
    if let Some(out) = stdout {
        output_parts.push(out.to_string());
    }

    if let Some(err) = stderr {
        output_parts.push(format!("Error: {}", err));
    }

    // Only add exit code if we already have some output (stdout or stderr)
    if !output_parts.is_empty() {
        if let Some(code) = exit_code.filter(|&c| c != 0) {
            output_parts.push(format!("Exit code: {}", code));
        }
    }

    // If we have output, return it
    if !output_parts.is_empty() {
        return Some(output_parts.join("\n"));
    }

    // Check for other result fields
    if let Some(result) = value
        .get("result")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        return Some(result);
    }

    // If command succeeded with no output, show a brief success message
    if success {
        if let Some(cmd) = command {
            return Some(format!("✓ {}", cmd));
        }
        // Try to extract tool name or other context from the response
        if let Some(tool_name) = value
            .get("tool_name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            return Some(format!("✓ {} executed successfully", tool_name));
        }
        if let Some(action) = value
            .get("action")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            return Some(format!("✓ {}", action));
        }
        return Some("✓ Operation completed successfully".to_string());
    }

    // If command failed with no output, show failure
    if let Some(cmd) = command {
        return Some(format!(
            "✗ {} (exit code: {})",
            cmd,
            exit_code.unwrap_or(-1)
        ));
    }

    // Check for error indicators
    if let Some(error) = value
        .get("error")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(format!("✗ {}", error));
    }

    if let Some(error_msg) = value
        .get("error_message")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(format!("✗ {}", error_msg));
    }

    Some("Command completed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_derive_output_with_stdout() {
        let tool_message = uni::Message::tool_response(
            "call_123".to_string(),
            json!({
                "stdout": "/Users/test/workspace\n",
                "stderr": "",
                "exit_code": 0,
                "command": "pwd"
            })
            .to_string(),
        );

        let history = vec![tool_message];
        let result = derive_recent_tool_output(&history);

        assert!(result.is_some());
        let output = result.unwrap();
        assert_eq!(output, "/Users/test/workspace");
    }

    #[test]
    fn test_derive_output_with_stderr() {
        let tool_message = uni::Message::tool_response(
            "call_123".to_string(),
            json!({
                "stdout": "",
                "stderr": "Error: file not found",
                "exit_code": 1,
                "command": "cat missing.txt"
            })
            .to_string(),
        );

        let history = vec![tool_message];
        let result = derive_recent_tool_output(&history);

        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.contains("Error: file not found"));
        assert!(output.contains("Exit code: 1"));
    }

    #[test]
    fn test_derive_output_no_output_success() {
        let tool_message = uni::Message::tool_response(
            "call_123".to_string(),
            json!({
                "stdout": "",
                "stderr": "",
                "exit_code": 0,
                "command": "touch file.txt"
            })
            .to_string(),
        );

        let history = vec![tool_message];
        let result = derive_recent_tool_output(&history);

        assert!(result.is_some());
        let output = result.unwrap();
        assert_eq!(output, "✓ touch file.txt");
    }

    #[test]
    fn test_derive_output_no_output_failure() {
        let tool_message = uni::Message::tool_response(
            "call_123".to_string(),
            json!({
                "stdout": "",
                "stderr": "",
                "exit_code": 127,
                "command": "nonexistent-command"
            })
            .to_string(),
        );

        let history = vec![tool_message];
        let result = derive_recent_tool_output(&history);

        assert!(result.is_some());
        let output = result.unwrap();
        assert_eq!(output, "✗ nonexistent-command (exit code: 127)");
    }

    #[test]
    fn test_derive_output_with_both_stdout_and_stderr() {
        let tool_message = uni::Message::tool_response(
            "call_123".to_string(),
            json!({
                "stdout": "Some output",
                "stderr": "Some warning",
                "exit_code": 0,
                "command": "test-command"
            })
            .to_string(),
        );

        let history = vec![tool_message];
        let result = derive_recent_tool_output(&history);

        assert!(result.is_some());
        let output = result.unwrap();
        assert!(output.contains("Some output"));
        assert!(output.contains("Error: Some warning"));
    }
}
