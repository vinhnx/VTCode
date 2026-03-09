use super::classify::extract_inline_backtick_command;
use super::paths::{normalize_path_operand, shell_quote_if_needed};

const RUN_COMMAND_PREFIX_WRAPPERS: [&str; 9] = [
    "unix command ",
    "shell command ",
    "command ",
    "cmd ",
    "please ",
    "kindly ",
    "just ",
    "quickly ",
    "quick ",
];

fn normalize_natural_language_command(command_part: &str) -> String {
    let extracted = extract_inline_backtick_command(command_part).unwrap_or(command_part);
    let cleaned = strip_run_command_prefixes(extracted);
    normalize_cargo_phrase(cleaned)
        .or_else(|| normalize_node_package_manager_phrase(cleaned))
        .or_else(|| normalize_pytest_phrase(cleaned))
        .or_else(|| normalize_unix_phrase(cleaned))
        .unwrap_or_else(|| cleaned.to_string())
}

pub(crate) fn strip_run_command_prefixes(command_part: &str) -> &str {
    let mut input = command_part.trim_start();
    loop {
        let lowered = input.to_ascii_lowercase();
        let stripped = RUN_COMMAND_PREFIX_WRAPPERS.iter().find_map(|prefix| {
            if lowered.starts_with(prefix) {
                input.get(prefix.len()..)
            } else {
                None
            }
        });
        let Some(rest) = stripped else {
            return input;
        };
        input = rest.trim_start();
    }
}

pub(super) fn normalize_for_shell_detection(command_part: &str) -> String {
    normalize_natural_language_command(command_part)
}

fn normalize_cargo_phrase(command_part: &str) -> Option<String> {
    let tokens: Vec<&str> = command_part.split_whitespace().collect();
    let lowered_tokens: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    if lowered_tokens.len() < 2 || lowered_tokens.first()? != "cargo" {
        return None;
    }

    match lowered_tokens.get(1).map(String::as_str) {
        Some("test") => normalize_cargo_test_phrase(&tokens, &lowered_tokens),
        Some("check") => normalize_cargo_check_phrase(&tokens, &lowered_tokens),
        _ => None,
    }
}

fn normalize_cargo_test_phrase(tokens: &[&str], lowered_tokens: &[String]) -> Option<String> {
    if lowered_tokens.len() >= 8
        && lowered_tokens[2] == "on"
        && lowered_tokens[4] == "bin"
        && lowered_tokens[5] == "for"
        && matches!(
            lowered_tokens[6].as_str(),
            "func" | "function" | "test" | "case"
        )
    {
        let bin_name = trim_token(tokens[3]);
        let test_name_joined = tokens[7..].join(" ");
        let test_name = trim_token(&test_name_joined);
        if !bin_name.is_empty() && !test_name.is_empty() {
            return Some(format!("cargo test --bin {} {}", bin_name, test_name));
        }
    }

    if lowered_tokens.len() >= 5
        && lowered_tokens[2] == "on"
        && matches!(lowered_tokens[4].as_str(), "package" | "pkg" | "crate")
    {
        let package_name = trim_token(tokens[3]);
        if !package_name.is_empty() {
            return Some(format!("cargo test -p {}", package_name));
        }
    }

    None
}

fn normalize_cargo_check_phrase(tokens: &[&str], lowered_tokens: &[String]) -> Option<String> {
    if lowered_tokens.len() >= 5
        && lowered_tokens[2] == "on"
        && matches!(lowered_tokens[4].as_str(), "package" | "pkg" | "crate")
    {
        let package_name = trim_token(tokens[3]);
        if !package_name.is_empty() {
            return Some(format!("cargo check -p {}", package_name));
        }
    }

    if lowered_tokens.len() >= 5 && lowered_tokens[2] == "on" && lowered_tokens[4] == "bin" {
        let bin_name = trim_token(tokens[3]);
        if !bin_name.is_empty() {
            return Some(format!("cargo check --bin {}", bin_name));
        }
    }

    None
}

fn normalize_node_package_manager_phrase(command_part: &str) -> Option<String> {
    let tokens: Vec<&str> = command_part.split_whitespace().collect();
    let lowered_tokens: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    if lowered_tokens.len() < 2 {
        return None;
    }

    let pm = lowered_tokens.first()?.as_str();
    if !matches!(pm, "npm" | "pnpm") {
        return None;
    }

    match lowered_tokens.get(1).map(String::as_str) {
        Some("test")
            if lowered_tokens.len() >= 5
                && lowered_tokens[2] == "on"
                && matches!(
                    lowered_tokens[4].as_str(),
                    "workspace" | "package" | "pkg" | "project"
                ) =>
        {
            let target = trim_token(tokens[3]);
            if target.is_empty() {
                return None;
            }
            Some(format!("{} test --workspace {}", pm, target))
        }
        Some("run")
            if lowered_tokens.len() >= 6
                && lowered_tokens[3] == "on"
                && matches!(
                    lowered_tokens[5].as_str(),
                    "workspace" | "package" | "pkg" | "project"
                ) =>
        {
            let script = trim_token(tokens[2]);
            let target = trim_token(tokens[4]);
            if script.is_empty() || target.is_empty() {
                return None;
            }
            Some(format!("{} run {} --workspace {}", pm, script, target))
        }
        _ => None,
    }
}

fn normalize_pytest_phrase(command_part: &str) -> Option<String> {
    let tokens: Vec<&str> = command_part.split_whitespace().collect();
    let lowered_tokens: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    if lowered_tokens.len() < 2 || lowered_tokens.first()? != "pytest" {
        return None;
    }

    if lowered_tokens.len() >= 6
        && lowered_tokens[1] == "on"
        && lowered_tokens[3] == "for"
        && matches!(
            lowered_tokens[4].as_str(),
            "func" | "function" | "test" | "case"
        )
    {
        let path = shell_quote_if_needed(&normalize_path_operand(tokens[2]));
        let test_name_joined = tokens[5..].join(" ");
        let test_name = trim_token(&test_name_joined);
        if path.is_empty() || test_name.is_empty() {
            return None;
        }
        return Some(format!("pytest {} -k {}", path, test_name));
    }

    if lowered_tokens.len() >= 3 && lowered_tokens[1] == "on" {
        let path_joined = tokens[2..].join(" ");
        let path = shell_quote_if_needed(&normalize_path_operand(&path_joined));
        if path.is_empty() {
            return None;
        }
        return Some(format!("pytest {}", path));
    }

    if lowered_tokens.len() >= 4
        && lowered_tokens[1] == "for"
        && matches!(
            lowered_tokens[2].as_str(),
            "func" | "function" | "test" | "case"
        )
    {
        let test_name_joined = tokens[3..].join(" ");
        let test_name = trim_token(&test_name_joined);
        if test_name.is_empty() {
            return None;
        }
        return Some(format!("pytest -k {}", test_name));
    }

    None
}

fn normalize_unix_phrase(command_part: &str) -> Option<String> {
    let tokens: Vec<&str> = command_part.split_whitespace().collect();
    let lowered_tokens: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    if lowered_tokens.len() < 2 {
        return None;
    }

    let cmd = lowered_tokens.first()?.as_str();
    let on_compatible_commands = [
        "ls", "cat", "head", "tail", "wc", "du", "df", "tree", "stat", "file", "bat", "less",
        "more", "git", "cargo", "pytest", "npm", "pnpm", "node", "python", "python3", "go", "java",
        "javac", "rustc", "make", "cmake", "docker", "kubectl",
    ];
    if lowered_tokens[1] == "on" && on_compatible_commands.contains(&cmd) {
        let target_joined = tokens[2..].join(" ");
        let target = shell_quote_if_needed(&normalize_path_operand(&target_joined));
        if !target.is_empty() {
            return Some(format!("{} {}", tokens[0], target));
        }
    }

    if matches!(cmd, "grep" | "rg")
        && lowered_tokens.len() >= 5
        && lowered_tokens[1] == "for"
        && let Some(on_idx) = lowered_tokens[2..].iter().position(|token| token == "on")
    {
        let on_idx = on_idx + 2;
        let pattern_joined = tokens[2..on_idx].join(" ");
        let pattern = trim_token(&pattern_joined);
        let target_joined = tokens[on_idx + 1..].join(" ");
        let target = shell_quote_if_needed(&normalize_path_operand(&target_joined));
        if !pattern.is_empty() && !target.is_empty() {
            return Some(format!("{} {} {}", tokens[0], pattern, target));
        }
    }

    if cmd == "find"
        && lowered_tokens.len() >= 5
        && lowered_tokens[1] == "on"
        && let Some(for_idx) = lowered_tokens[2..].iter().position(|token| token == "for")
    {
        let for_idx = for_idx + 2;
        let base_joined = tokens[2..for_idx].join(" ");
        let base = shell_quote_if_needed(&normalize_path_operand(&base_joined));
        let pattern_joined = tokens[for_idx + 1..].join(" ");
        let pattern = trim_token(&pattern_joined);
        if !base.is_empty() && !pattern.is_empty() {
            return Some(format!("find {} -name {}", base, pattern));
        }
    }

    None
}

fn trim_token(token: &str) -> &str {
    token.trim().trim_end_matches(['.', ',', ';', '!', '?'])
}
