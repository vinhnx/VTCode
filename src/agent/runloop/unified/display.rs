use crate::agent::runloop::unified::shell::{
    detect_explicit_run_command, strip_run_command_prefixes,
};
use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, Reset, Style as AnsiStyle};
use anyhow::{Context, Result};
use vtcode_core::command_safety::shell_parser::parse_shell_commands_tree_sitter;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::update_theme_preference;

pub(crate) async fn persist_theme_preference(
    renderer: &mut AnsiRenderer,
    theme_id: &str,
) -> Result<()> {
    if let Err(err) = update_theme_preference(theme_id).await {
        renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist theme preference: {}", err),
        )?;
    }
    if let Err(err) = persist_theme_config(theme_id) {
        renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist theme in vtcode.toml: {}", err),
        )?;
    }
    Ok(())
}

fn persist_theme_config(theme_id: &str) -> Result<()> {
    let mut manager =
        ConfigManager::load().context("Failed to load configuration for theme update")?;
    let mut config = manager.config().clone();
    if config.agent.theme != theme_id {
        config.agent.theme = theme_id.to_string();
        manager
            .save_config(&config)
            .context("Failed to save theme to configuration")?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn ensure_turn_bottom_gap(
    renderer: &mut AnsiRenderer,
    applied: &mut bool,
) -> Result<()> {
    if !*applied {
        renderer.line_if_not_empty(MessageStyle::Output)?;
        *applied = true;
    }
    Ok(())
}

/// Display a user message using the active user styling
pub(crate) fn display_user_message(renderer: &mut AnsiRenderer, message: &str) -> Result<()> {
    let rendered = highlight_shell_user_input(message).unwrap_or_else(|| message.to_string());
    renderer.line(MessageStyle::User, &rendered)
}

fn is_bash_keyword(token: &str) -> bool {
    matches!(
        token,
        "if" | "then"
            | "else"
            | "elif"
            | "fi"
            | "for"
            | "in"
            | "do"
            | "done"
            | "while"
            | "until"
            | "case"
            | "esac"
            | "function"
            | "select"
            | "time"
            | "coproc"
            | "{"
            | "}"
            | "[["
            | "]]"
    )
}

fn is_command_separator(token: &str) -> bool {
    matches!(token, "|" | "||" | "&&" | ";" | ";;" | "&")
}

fn tokenize_preserve_whitespace(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut token_start: Option<usize> = None;
    let mut token_is_whitespace = false;

    for (idx, ch) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' && !in_single {
            escaped = true;
        } else if ch == '\'' && !in_double {
            in_single = !in_single;
        } else if ch == '"' && !in_single {
            in_double = !in_double;
        }

        let is_whitespace = !in_single && !in_double && ch.is_whitespace();
        match token_start {
            None => {
                token_start = Some(idx);
                token_is_whitespace = is_whitespace;
            }
            Some(start) if token_is_whitespace != is_whitespace => {
                parts.push(&text[start..idx]);
                token_start = Some(idx);
                token_is_whitespace = is_whitespace;
            }
            _ => {}
        }
    }

    if let Some(start) = token_start {
        parts.push(&text[start..]);
    }

    parts
}

fn style_for_token(token: &str, expect_command: &mut bool) -> Option<AnsiStyle> {
    if token.trim().is_empty() {
        return None;
    }
    if is_command_separator(token) {
        *expect_command = true;
        return None;
    }
    if token.starts_with('"')
        || token.starts_with('\'')
        || token.ends_with('"')
        || token.ends_with('\'')
    {
        *expect_command = false;
        return Some(AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Yellow))));
    }
    if token.starts_with('$') || token.contains("=$") || token.starts_with("${") {
        *expect_command = false;
        return Some(AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Yellow))));
    }
    if token.starts_with('-') && token.len() > 1 {
        *expect_command = false;
        return Some(AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Red))));
    }
    if is_bash_keyword(token) {
        *expect_command = true;
        return Some(
            AnsiStyle::new()
                .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Blue)))
                .effects(Effects::BOLD),
        );
    }
    if *expect_command {
        *expect_command = false;
        return Some(
            AnsiStyle::new()
                .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Green)))
                .effects(Effects::BOLD),
        );
    }
    Some(
        AnsiStyle::new()
            .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::White)))
            .effects(Effects::DIMMED),
    )
}

fn strip_matching_backticks(input: &str) -> &str {
    let trimmed = input.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('`') && trimmed.ends_with('`') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        input
    }
}

fn highlight_shell_command(command: &str) -> String {
    let command = strip_matching_backticks(command);
    if !is_valid_bash_grammar(command) {
        return command.to_string();
    }
    let mut rendered = String::with_capacity(command.len() + 32);
    let mut expect_command = true;
    for token in tokenize_preserve_whitespace(command) {
        if let Some(style) = style_for_token(token, &mut expect_command) {
            rendered.push_str(&style.to_string());
            rendered.push_str(token);
            rendered.push_str(&Reset.to_string());
        } else {
            rendered.push_str(token);
        }
    }
    rendered
}

fn is_valid_bash_grammar(command: &str) -> bool {
    parse_shell_commands_tree_sitter(command)
        .map(|commands| !commands.is_empty())
        .unwrap_or(false)
}

fn highlight_shell_user_input(message: &str) -> Option<String> {
    let leading_ws_len = message.chars().take_while(|ch| ch.is_whitespace()).count();
    let leading_ws_bytes = message
        .char_indices()
        .nth(leading_ws_len)
        .map(|(idx, _)| idx)
        .unwrap_or(message.len());
    let trimmed = &message[leading_ws_bytes..];

    if let Some(rest) = trimmed.strip_prefix('!') {
        let command = rest.trim();
        if command.is_empty() || !is_valid_bash_grammar(strip_matching_backticks(command)) {
            return None;
        }
        let prefix_len = rest.len() - rest.trim_start().len();
        let prefix_style = AnsiStyle::new()
            .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::White)))
            .effects(Effects::DIMMED);
        let prefix = format!(
            "{}{}!{}{}",
            prefix_style,
            &message[..leading_ws_bytes],
            &rest[..prefix_len],
            Reset
        );
        return Some(format!("{}{}", prefix, highlight_shell_command(command)));
    }

    if let Some((prefix_end, command)) = extract_run_command_for_highlight(trimmed) {
        detect_explicit_run_command(trimmed)?;
        if !is_valid_bash_grammar(strip_matching_backticks(command)) {
            return None;
        }
        let prefix = &trimmed[..prefix_end];
        let prefix_style = AnsiStyle::new()
            .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::White)))
            .effects(Effects::DIMMED);
        let prefix_rendered = format!(
            "{}{}{}{}",
            prefix_style,
            &message[..leading_ws_bytes],
            prefix,
            Reset
        );
        return Some(format!(
            "{}{}",
            prefix_rendered,
            highlight_shell_command_preserve_text(command)
        ));
    }

    None
}

fn highlight_shell_command_preserve_text(command: &str) -> String {
    let trimmed = command.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('`') && trimmed.ends_with('`') {
        let leading_len = command.len() - command.trim_start().len();
        let trailing_len = command.len() - command.trim_end().len();
        let leading = &command[..leading_len];
        let trailing = &command[command.len() - trailing_len..];
        let inner = &trimmed[1..trimmed.len() - 1];
        return format!(
            "{}`{}`{}",
            leading,
            highlight_shell_command(inner),
            trailing
        );
    }
    highlight_shell_command(command)
}

fn extract_run_command_for_highlight(input: &str) -> Option<(usize, &str)> {
    if !input.to_ascii_lowercase().starts_with("run ") {
        return None;
    }

    let mut index = 3usize;
    while let Some(ch) = input[index..].chars().next() {
        if !ch.is_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }
    if index >= input.len() {
        return None;
    }

    let command = strip_run_command_prefixes(&input[index..]);
    if command.is_empty() {
        return None;
    }

    let command_start = input.len().saturating_sub(command.len());
    Some((command_start, command))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::utils::ansi_parser::strip_ansi;

    #[test]
    fn highlights_run_prefix_user_input() {
        let highlighted = highlight_shell_user_input("run cargo fmt").expect("should highlight");
        assert_eq!(strip_ansi(&highlighted), "run cargo fmt");
        assert!(highlighted.contains("cargo"));
        assert!(highlighted.contains("fmt"));
    }

    #[test]
    fn highlights_bang_prefix_user_input() {
        let highlighted = highlight_shell_user_input("!echo $HOME").expect("should highlight");
        assert!(highlighted.contains("!"));
        assert!(highlighted.contains("echo"));
        assert!(highlighted.contains("$HOME"));
    }

    #[test]
    fn skips_natural_language_run_input() {
        assert!(highlight_shell_user_input("run the tests").is_none());
    }

    #[test]
    fn strips_backticks_from_explicit_run_command() {
        let highlighted = highlight_shell_user_input("run `cargo fmt`").expect("should highlight");
        assert_eq!(strip_ansi(&highlighted), "run `cargo fmt`");
        assert!(highlighted.contains("cargo"));
        assert!(highlighted.contains("fmt"));
    }

    #[test]
    fn preserves_text_with_unix_command_wrapper() {
        let highlighted =
            highlight_shell_user_input("run unix command ls -la").expect("should highlight");
        assert_eq!(strip_ansi(&highlighted), "run unix command ls -la");
    }

    #[test]
    fn preserves_text_with_mixed_wrappers() {
        let highlighted =
            highlight_shell_user_input("run command please cargo check").expect("should highlight");
        assert_eq!(strip_ansi(&highlighted), "run command please cargo check");
    }

    #[test]
    fn skips_highlighting_for_invalid_bash_grammar() {
        assert!(highlight_shell_user_input("run )(").is_none());
        assert!(highlight_shell_user_input("! )(").is_none());
    }
}
