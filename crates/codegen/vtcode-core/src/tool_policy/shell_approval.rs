use super::{KNOWN_MUTATING_COMMANDS, MUTATING_OPTION_HINTS, SHELL_APPROVAL_SCOPE_MARKER};

pub(crate) fn split_shell_approval_entry(entry: &str) -> (&str, Option<&str>) {
    if let Some(index) = entry.find(SHELL_APPROVAL_SCOPE_MARKER) {
        let (prefix, scoped) = entry.split_at(index);
        (prefix, Some(&scoped[1..]))
    } else {
        (entry, None)
    }
}

pub(crate) fn shell_words_lossy(text: &str) -> Vec<String> {
    shell_words::split(text).unwrap_or_else(|_| text.split_whitespace().map(str::to_owned).collect())
}

fn shell_command_words_for_approval(text: &str) -> Vec<String> {
    crate::command_safety::shell_parser::parse_shell_commands_tree_sitter(text)
        .ok()
        .and_then(|commands| {
            if commands.len() == 1 {
                commands.into_iter().next()
            } else {
                None
            }
        })
        .filter(|command| !command.is_empty())
        .unwrap_or_else(|| shell_words_lossy(text))
}

fn is_probable_workspace_path(word: &str) -> bool {
    if word.is_empty() || word.starts_with('-') || word.starts_with('~') || word == "." {
        return false;
    }
    let trimmed = word.trim_end_matches('/');
    if trimmed.is_empty() {
        return false;
    }

    let parts = if trimmed.starts_with('/') {
        trimmed.split('/').skip(1).collect::<Vec<_>>()
    } else {
        trimmed.split('/').collect::<Vec<_>>()
    };

    !parts.is_empty()
        && parts
            .iter()
            .all(|part| !part.is_empty() && *part != "." && *part != ".." && !part.contains('\0'))
}

fn command_looks_like_readonly_path_query(program: &str, words: &[String]) -> bool {
    if program.is_empty()
        || KNOWN_MUTATING_COMMANDS.contains(&program)
        || words.iter().skip(1).any(|word| MUTATING_OPTION_HINTS.contains(&word.as_str()))
    {
        return false;
    }

    words.iter().skip(1).any(|word| is_probable_workspace_path(word))
}

fn command_path_args<'a>(program: &str, words: &'a [String]) -> Vec<&'a str> {
    if !command_looks_like_readonly_path_query(program, words) {
        return Vec::new();
    }

    words
        .iter()
        .skip(1)
        .filter(|word| is_probable_workspace_path(word))
        .map(String::as_str)
        .collect()
}

pub(crate) fn derived_shell_approval_prefixes(key: &str) -> Vec<String> {
    let (command_text, scope_signature) = split_shell_approval_entry(key);
    // Opaque learned-family keys (`shell-pattern:<program> <family>`) are not
    // shell command text. Running the shell tokenizer on them would treat
    // `shell-pattern:awk README.md` as program `shell-pattern:awk` and derive a
    // `shell-pattern:awk` prefix that matches *every* awk family — silently
    // widening a scoped family approval. Family keys are matched by their exact
    // persisted value, so derive nothing.
    if command_text.starts_with("shell-pattern:") {
        return Vec::new();
    }
    let words = shell_command_words_for_approval(command_text);
    if words.len() < 2 {
        return Vec::new();
    }

    let append_scope = |prefix: String| {
        if let Some(scope_signature) = scope_signature {
            format!("{prefix}|{scope_signature}")
        } else {
            prefix
        }
    };

    let mut prefixes = Vec::new();
    if let Some(program) = words.first().map(String::as_str) {
        let path_args = command_path_args(program, &words);
        if !path_args.is_empty() {
            let option_words = words[1..]
                .iter()
                .filter(|word| !is_probable_workspace_path(word))
                .map(String::as_str)
                .collect::<Vec<_>>();
            let mut prefix_words = Vec::with_capacity(1 + option_words.len());
            prefix_words.push(program);
            prefix_words.extend(option_words);
            prefixes.push(append_scope(shell_words::join(prefix_words)));
            prefixes.push(append_scope(program.to_string()));
        }

        match program {
            "sed" if words.len() >= 3 && words.get(1).is_some_and(|arg| arg == "-n") => {
                prefixes.push(append_scope("sed -n".to_string()));
            }
            "cargo" | "git" if words.len() >= 2 => {
                prefixes.push(append_scope(shell_words::join(words[..2].iter().map(String::as_str))));
            }
            _ => {}
        }
    }

    prefixes.sort();
    prefixes.dedup();
    prefixes
}

pub(crate) fn shell_command_words_match_prefix(command_words: &[String], prefix_words: &[String]) -> bool {
    command_words.len() >= prefix_words.len()
        && prefix_words
            .iter()
            .zip(command_words.iter())
            .all(|(prefix, command)| prefix == command)
}
