use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider as uni;

pub(crate) fn should_short_circuit_shell(
    input: &str,
    tool_name: &str,
    args: &serde_json::Value,
) -> bool {
    if tool_name != tools::RUN_TERMINAL_CMD && tool_name != tools::BASH {
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

    let value = serde_json::from_str::<serde_json::Value>(&message.content).ok()?;

    let mut output_parts = Vec::new();

    if let Some(stdout) = value
        .get("stdout")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        output_parts.push(format!("Output:\n{}", stdout));
    }

    if let Some(stderr) = value
        .get("stderr")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        output_parts.push(format!("Errors:\n{}", stderr));
    }

    if let Some(exit_code) = value.get("exit_code").and_then(|v| v.as_i64()) {
        if exit_code != 0 {
            output_parts.push(format!("Exit code: {}", exit_code));
        }
    }

    if let Some(command) = value
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        output_parts.push(format!("Command executed: {}", command));
    }

    if output_parts.is_empty() {
        if let Some(result) = value
            .get("result")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            return Some(result);
        }
        return Some("Command completed successfully.".to_string());
    }

    Some(output_parts.join("\n\n"))
}
