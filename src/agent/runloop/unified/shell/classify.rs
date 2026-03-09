use shell_words::split as shell_split;
use vtcode_core::command_safety::shell_parser::parse_shell_commands_tree_sitter;

pub(super) fn looks_like_natural_language_request(command_part: &str) -> bool {
    let natural_language_indicators = [
        "the ", "all ", "some ", "this ", "that ", "my ", "our ", "a ", "an ",
    ];
    let command_lower = command_part.to_ascii_lowercase();
    natural_language_indicators
        .iter()
        .any(|indicator| command_lower.starts_with(indicator))
}

pub(super) fn looks_like_shell_command(command_part: &str) -> bool {
    parse_shell_commands_tree_sitter(command_part)
        .map(|commands| !commands.is_empty())
        .unwrap_or_else(|_| {
            shell_split(command_part)
                .map(|tokens| !tokens.is_empty())
                .unwrap_or(false)
        })
}

pub(super) fn extract_inline_backtick_command(command_part: &str) -> Option<&str> {
    let start = command_part.find('`')?;
    let remainder = command_part.get(start + 1..)?;
    let end_rel = remainder.find('`')?;
    let extracted = remainder.get(..end_rel)?.trim();
    if extracted.is_empty() {
        return None;
    }
    Some(extracted)
}

pub(super) fn contains_chained_instruction(command_part: &str) -> bool {
    let Ok(tokens) = shell_split(command_part) else {
        return false;
    };
    if tokens.len() < 2 {
        return false;
    }

    let separators = ["and", "then", "after", "before", "also", "next"];
    for (idx, token) in tokens.iter().enumerate() {
        let lowered = token.to_ascii_lowercase();
        if separators.contains(&lowered.as_str()) && idx + 1 < tokens.len() {
            return true;
        }
    }

    false
}
