#![allow(missing_docs)]
use proptest::prelude::*;
use vtcode_core::command_safety::dangerous_commands::command_might_be_dangerous;
use vtcode_core::command_safety::shell_parser::parse_shell_commands;
use vtcode_core::exec_policy::{Decision, Policy, PrefixRule};
use vtcode_core::tools::validation::paths::{validate_non_root_listing_path, validate_path_safety};

fn to_strings(args: Vec<String>) -> Vec<String> {
    args
}

proptest! {
    /// Fuzz: `command_might_be_dangerous` must never panic on any input
    #[test]
    fn fuzz_dangerous_commands_never_panics(
        cmd0 in ".{0,64}",
        cmd1 in ".{0,64}",
        cmd2 in ".{0,64}",
        cmd3 in ".{0,64}",
        cmd4 in ".{0,64}",
    ) {
        let command = to_strings(vec![cmd0, cmd1, cmd2, cmd3, cmd4]);
        // Must not panic regardless of input
        let _ = command_might_be_dangerous(&command);
    }
}

proptest! {
    /// Fuzz: `command_might_be_dangerous` with variable-length commands
    #[test]
    fn fuzz_dangerous_commands_variable_length(
        args in prop::collection::vec("\\PC{0,32}", 0..10),
    ) {
        let command = to_strings(args);
        let _ = command_might_be_dangerous(&command);
    }
}

proptest! {
    /// Fuzz: `parse_shell_commands` must never panic on any input
    #[test]
    fn fuzz_shell_parser_never_panics(
        script in "\\PC{0,512}",
    ) {
        let _ = parse_shell_commands(&script);
    }
}

proptest! {
    /// Fuzz: `PrefixRule::matches` must never panic
    #[test]
    fn fuzz_prefix_rule_matches_never_panics(
        pattern_len in 0usize..5,
        pattern_words in prop::collection::vec("\\PC{0,16}", 0..5),
        command_words in prop::collection::vec("\\PC{0,16}", 0..8),
    ) {
        let pattern: Vec<String> = pattern_words.into_iter().take(pattern_len).collect();
        let rule = PrefixRule::new(pattern, Decision::Allow);
        let _ = rule.matches(&command_words.to_vec());
    }
}

proptest! {
    /// Fuzz: `Policy::check` must never panic
    #[test]
    fn fuzz_policy_check_never_panics(
        rules_count in 0usize..5,
        commands_count in 0usize..5,
    ) {
        let mut policy = Policy::empty();
        for _ in 0..rules_count {
            let pat: Vec<String> = (0..3).map(|_| "[a-zA-Z]{0,8}".to_string()).collect();
            let parsed: Vec<String> = pat.iter().map(|_| "test".to_string()).collect();
            let _ = policy.add_prefix_rule(&parsed, Decision::Allow);
        }
        for _ in 0..commands_count {
            let cmd: Vec<String> = (0..4).map(|_| "test".to_string()).collect();
            let _ = policy.check(&cmd);
        }
    }
}

proptest! {
    /// Fuzz: `validate_path_safety` must never panic on any input
    #[test]
    fn fuzz_path_validation_never_panics(
        path in "\\PC{0,256}",
    ) {
        let _ = validate_path_safety(&path);
    }
}

proptest! {
    /// Fuzz: `validate_non_root_listing_path` must never panic
    #[test]
    fn fuzz_non_root_listing_path_never_panics(
        path in "\\PC{0,128}",
    ) {
        if !path.is_empty() {
            let _ = validate_non_root_listing_path(Some(&path));
        }
        let _ = validate_non_root_listing_path(None);
    }
}

proptest! {
    /// Fuzz: `Policy::check_multiple` must never panic with arbitrary rule sets
    #[test]
    fn fuzz_policy_check_multiple_never_panics(
        pattern_count in 0usize..8,
        command_count in 0usize..6,
        command_word_count in 0usize..5,
    ) {
        let mut policy = Policy::empty();
        for _ in 0..pattern_count {
            let pat_len = 1 + (pattern_count % 3);
            let pat: Vec<String> = (0..pat_len).map(|i| format!("cmd{i}")).collect();
            let dec = match pattern_count % 3 {
                0 => Decision::Allow,
                1 => Decision::Prompt,
                _ => Decision::Forbidden,
            };
            let _ = policy.add_prefix_rule(&pat, dec);
        }

        let cmds: Vec<Vec<String>> = (0..command_count)
            .map(|i| {
                (0..command_word_count).map(|j| format!("arg{}", j + i * 10)).collect()
            })
            .collect();

        let _ = policy.check_multiple(cmds.iter(), &|_| Decision::Prompt);
    }
}

proptest! {
    /// Invariant: `PrefixRule::matches` is prefix-only (not substring or suffix)
    #[test]
    fn fuzz_prefix_rule_invariant(command_words in prop::collection::vec("\\PC{1,8}", 1..6)) {
        let empty_rule = PrefixRule::new(vec![], Decision::Allow);
        prop_assert!(empty_rule.matches(&command_words));

        let rule = PrefixRule::new(
            command_words.iter().take(1).cloned().collect(),
            Decision::Allow,
        );
        prop_assert!(rule.matches(&command_words));
    }
}

proptest! {
    /// Invariant: longer pattern than command never matches
    #[test]
    fn fuzz_prefix_rule_longer_pattern_never_matches(
        pattern in prop::collection::vec("\\PC{1,8}", 2..5),
        cmd_word in "\\PC{1,8}",
    ) {
        let rule = PrefixRule::new(pattern.clone(), Decision::Allow);
        let short_cmd: Vec<String> = vec![cmd_word];
        prop_assert!(!rule.matches(&short_cmd));
    }
}

/// Invariant: `command_might_be_dangerous` with empty command is always safe
#[test]
fn fuzz_empty_command_is_safe() {
    let empty: Vec<String> = vec![];
    assert!(!command_might_be_dangerous(&empty));
}

/// Known invariants for dangerous commands
#[test]
fn fuzz_known_dangerous_commands() {
    let dangerous_cases = vec![
        vec!["git", "reset"],
        vec!["git", "reset", "--hard"],
        vec!["git", "rm"],
        vec!["git", "branch", "-d", "feature"],
        vec!["git", "branch", "-D", "feature"],
        vec!["git", "push", "--force"],
        vec!["git", "push", "-f"],
        vec!["git", "clean", "-fdx"],
        vec!["rm", "-f", "file"],
        vec!["rm", "-rf", "/"],
        vec!["mkfs", "/dev/sda"],
        vec!["mkfs.ext4", "/dev/sda1"],
        vec!["dd", "if=/dev/zero", "of=/dev/sda"],
        vec!["shutdown"],
        vec!["sudo", "rm", "-rf", "/"],
    ];
    for cmd in dangerous_cases {
        let command: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
        assert!(command_might_be_dangerous(&command), "expected dangerous: {cmd:?}");
    }
}

#[test]
fn fuzz_known_safe_commands() {
    let safe_cases = vec![
        vec!["git", "status"],
        vec!["git", "log"],
        vec!["git", "diff"],
        vec!["git", "push", "origin", "main"],
        vec!["git", "checkout", "reset"],
        vec!["ls", "-la"],
        vec!["echo", "hello"],
        vec!["rm", "file.txt"],
        vec!["cat", "README.md"],
    ];
    for cmd in safe_cases {
        let command: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
        assert!(!command_might_be_dangerous(&command), "expected safe: {cmd:?}");
    }
}
