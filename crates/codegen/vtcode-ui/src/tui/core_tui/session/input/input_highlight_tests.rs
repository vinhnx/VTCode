use super::*;

fn kinds(input: &str) -> Vec<(InputTokenKind, String)> {
    tokenize_input(input)
        .into_iter()
        .map(|t| {
            let text: String = input.chars().skip(t.start).take(t.end - t.start).collect();
            (t.kind, text)
        })
        .collect()
}

#[test]
fn slash_command_at_start() {
    let tokens = kinds("/use skill-name");
    assert_eq!(tokens[0].0, InputTokenKind::SlashCommand);
    assert_eq!(tokens[0].1, "/use");
    assert_eq!(tokens[1].0, InputTokenKind::Normal);
}

#[test]
fn slash_command_with_following_text() {
    let tokens = kinds("/checkup hello");
    assert_eq!(tokens[0].0, InputTokenKind::SlashCommand);
    assert_eq!(tokens[0].1, "/checkup");
    assert_eq!(tokens[1].0, InputTokenKind::Normal);
}

#[test]
fn at_file_reference() {
    let tokens = kinds("check @src/main.rs please");
    assert_eq!(tokens[0].0, InputTokenKind::Normal);
    assert_eq!(tokens[1].0, InputTokenKind::FileReference);
    assert_eq!(tokens[1].1, "@src/main.rs");
    assert_eq!(tokens[2].0, InputTokenKind::Normal);
}

#[test]
fn inline_backtick_code() {
    let tokens = kinds("run `cargo test` now");
    assert_eq!(tokens[0].0, InputTokenKind::Normal);
    assert_eq!(tokens[1].0, InputTokenKind::InlineCode);
    assert_eq!(tokens[1].1, "`cargo test`");
    assert_eq!(tokens[2].0, InputTokenKind::Normal);
}

#[test]
fn no_false_slash_mid_word() {
    let tokens = kinds("path/to/file");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, InputTokenKind::Normal);
}

#[test]
fn empty_input() {
    assert!(tokenize_input("").is_empty());
}

#[test]
fn mixed_tokens() {
    let tokens = kinds("/use @file.rs `code`");
    assert_eq!(tokens[0].0, InputTokenKind::SlashCommand);
    assert_eq!(tokens[2].0, InputTokenKind::FileReference);
    assert_eq!(tokens[4].0, InputTokenKind::InlineCode);
}

#[test]
fn agent_reference_has_dedicated_token_kind() {
    let tokens = kinds("use @agent-explorer for this");
    assert_eq!(tokens[1].0, InputTokenKind::AgentReference);
    assert_eq!(tokens[1].1, "@agent-explorer");
}

#[test]
fn byte_index_to_char_index_is_char_boundary_safe() {
    // Multi-byte content: "a→b" is [a, →(3 bytes), b] = 5 bytes.
    let content = "a→b";
    assert_eq!(byte_index_to_char_index(content, 0), 0);
    // Byte indices inside the 3-byte '→' clamp to its start (char 1).
    assert_eq!(byte_index_to_char_index(content, 2), 1);
    // Byte index 4 is the start of 'b' (char 2).
    assert_eq!(byte_index_to_char_index(content, 4), 2);
    // Out-of-range indices clamp to the end (3 chars).
    assert_eq!(byte_index_to_char_index(content, 99), 3);
}

#[test]
fn plugin_agent_reference_has_dedicated_token_kind() {
    let tokens = kinds("use @agent-github:reviewer for this");
    assert_eq!(tokens[1].0, InputTokenKind::AgentReference);
    assert_eq!(tokens[1].1, "@agent-github:reviewer");
}
