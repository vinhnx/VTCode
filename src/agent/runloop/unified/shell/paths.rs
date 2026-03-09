pub(super) fn normalize_path_operand(target: &str) -> String {
    let normalized = strip_optional_word_prefix(trim_wrapping_quotes_and_punctuation(target), "on");
    let normalized = trim_wrapping_quotes_and_punctuation(normalized);
    let normalized = normalized.strip_prefix('@').unwrap_or(normalized);
    trim_wrapping_quotes_and_punctuation(normalized).to_string()
}

pub(super) fn trim_wrapping_quotes_and_punctuation(target: &str) -> &str {
    let mut normalized = target.trim();
    loop {
        let previous = normalized;
        normalized = normalized.trim();
        normalized = normalized.strip_prefix('"').unwrap_or(normalized);
        normalized = normalized.strip_suffix('"').unwrap_or(normalized);
        normalized = normalized.strip_prefix('\'').unwrap_or(normalized);
        normalized = normalized.strip_suffix('\'').unwrap_or(normalized);
        normalized = normalized
            .trim_end_matches(['.', ',', ';', '!', '?'])
            .trim();
        if normalized == previous {
            return normalized;
        }
    }
}

pub(super) fn strip_optional_word_prefix<'a>(input: &'a str, word: &str) -> &'a str {
    let Some(prefix) = input.get(..word.len()) else {
        return input;
    };

    if !prefix.eq_ignore_ascii_case(word) {
        return input;
    }

    let remainder = &input[word.len()..];
    if remainder.chars().next().is_some_and(char::is_whitespace) {
        remainder.trim_start()
    } else {
        input
    }
}

pub(crate) fn shell_quote_if_needed(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    if value.chars().all(is_shell_safe_unquoted_char) {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', r#"'\''"#))
}

fn is_shell_safe_unquoted_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | '~' | ':')
}
