#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::Path;
use vtcode_core::exec_policy::{Policy, PolicyParser};

const MAX_INPUT_BYTES: usize = 4096;

fn bounded_input(data: &[u8]) -> String {
    let slice = if data.len() > MAX_INPUT_BYTES {
        &data[..MAX_INPUT_BYTES]
    } else {
        data
    };
    String::from_utf8_lossy(slice).into_owned()
}

fn add_rules_to_policy(
    patterns: impl Iterator<Item = (String, vtcode_core::exec_policy::Decision)>,
) {
    let mut policy = Policy::empty();
    for (pattern, decision) in patterns {
        let parsed: Vec<String> = pattern
            .split_whitespace()
            .map(ToString::to_string)
            .collect();
        let _ = policy.add_prefix_rule(&parsed, decision);
    }
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let mode = data[0] % 3;
    let input = bounded_input(&data[1..]);
    let parser = PolicyParser::new();

    match mode {
        0 => {
            if let Ok(rules) = parser.parse_simple(&input) {
                let iter = rules.into_iter().map(|rule| {
                    let joined = rule.pattern.join(" ");
                    (joined, rule.decision)
                });
                add_rules_to_policy(iter);
            }

            let _ = parser.load_from_content(&input, Path::new("policy.rules"));
        }
        1 => {
            if let Ok(file) = parser.parse_toml(&input) {
                let iter = file
                    .rules
                    .into_iter()
                    .map(|rule| (rule.pattern, rule.decision));
                add_rules_to_policy(iter);
            }

            let _ = parser.load_from_content(&input, Path::new("policy.toml"));
        }
        _ => {
            if let Ok(file) = parser.parse_json(&input) {
                let iter = file
                    .rules
                    .into_iter()
                    .map(|rule| (rule.pattern, rule.decision));
                add_rules_to_policy(iter);
            }

            let _ = parser.load_from_content(&input, Path::new("policy.json"));
        }
    }
});
