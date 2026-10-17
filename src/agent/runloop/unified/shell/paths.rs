pub(super) fn normalize_path_operand(target: &str) -> String {
    use vtcode_commons::formatting::{strip_optional_word_prefix, trim_wrapping_quotes_and_punctuation};

    let normalized = strip_optional_word_prefix(trim_wrapping_quotes_and_punctuation(target), "on");
    let normalized = trim_wrapping_quotes_and_punctuation(normalized);
    let normalized = normalized.strip_prefix('@').unwrap_or(normalized);
    trim_wrapping_quotes_and_punctuation(normalized).to_string()
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
