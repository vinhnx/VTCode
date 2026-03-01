#![no_main]

use libfuzzer_sys::fuzz_target;
use vtcode_core::command_safety::shell_parser::{
    parse_bash_lc_commands, parse_shell_commands, parse_shell_commands_tree_sitter,
};

const MAX_INPUT_BYTES: usize = 2048;
const MAX_TOKENS: usize = 64;

fn bounded_input(data: &[u8]) -> String {
    let slice = if data.len() > MAX_INPUT_BYTES {
        &data[..MAX_INPUT_BYTES]
    } else {
        data
    };
    String::from_utf8_lossy(slice).into_owned()
}

fn assert_no_empty_tokens(commands: Vec<Vec<String>>) {
    for command in commands {
        if command.is_empty() {
            continue;
        }
        for token in command {
            assert!(!token.trim().is_empty(), "parser returned empty token");
        }
    }
}

fn tokenized_invocation(script: &str) -> Vec<String> {
    script
        .split_whitespace()
        .take(MAX_TOKENS)
        .map(ToString::to_string)
        .collect()
}

fuzz_target!(|data: &[u8]| {
    let script = bounded_input(data);

    if let Ok(commands) = parse_shell_commands(&script) {
        assert_no_empty_tokens(commands);
    }

    if let Ok(commands) = parse_shell_commands_tree_sitter(&script) {
        assert_no_empty_tokens(commands);
    }

    let bash_lc = vec!["bash".to_string(), "-lc".to_string(), script.clone()];
    if let Some(commands) = parse_bash_lc_commands(&bash_lc) {
        assert_no_empty_tokens(commands);
    }

    let raw_tokens = tokenized_invocation(&script);
    if let Some(commands) = parse_bash_lc_commands(&raw_tokens) {
        assert_no_empty_tokens(commands);
    }
});
