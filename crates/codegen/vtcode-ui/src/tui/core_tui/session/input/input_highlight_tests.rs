use super::*;
use crate::tui::core_tui::types::InlineTheme;
use anstyle::RgbColor;

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

/// Render a single draft under a theme and return the style of one token span.
fn token_span_style(input: &str, theme: InlineTheme, needle: &str) -> Style {
    let mut session = Session::new(theme, None, 4);
    session.set_input(input.to_string());
    session.set_cursor(input.len());
    let render = session.build_input_render_for_test(80, 4);
    render
        .text
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.as_ref() == needle)
        .map(|span| span.style)
        .unwrap_or_else(|| panic!("no rendered span for {needle:?}"))
}

#[test]
fn prompt_token_colors_come_from_theme_slots() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x11, 0x22, 0x33))),
        secondary: Some(AnsiColorEnum::Rgb(RgbColor(0x44, 0x55, 0x66))),
        tool_accent: Some(AnsiColorEnum::Rgb(RgbColor(0x77, 0x88, 0x99))),
        warning: Some(AnsiColorEnum::Rgb(RgbColor(0xAA, 0xBB, 0xCC))),
        ..Default::default()
    };
    let input = "/goal @agent-x @README.md `code`";
    let expected = |color| Some(ratatui_color_from_ansi(color));

    let slash = token_span_style(input, theme.clone(), "/goal");
    assert_eq!(slash.fg, expected(theme.primary.unwrap()));
    assert!(slash.add_modifier.contains(Modifier::BOLD));

    let agent = token_span_style(input, theme.clone(), "@agent-x");
    assert_eq!(agent.fg, expected(theme.tool_accent.unwrap()));
    assert!(agent.add_modifier.contains(Modifier::UNDERLINED));

    let file = token_span_style(input, theme.clone(), "@README.md");
    assert_eq!(file.fg, expected(theme.secondary.unwrap()));
    assert!(file.add_modifier.contains(Modifier::UNDERLINED));

    let code = token_span_style(input, theme.clone(), "`code`");
    assert_eq!(code.fg, expected(theme.warning.unwrap()));
    assert!(code.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn prompt_token_colors_change_with_theme() {
    let dark = InlineTheme {
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x11, 0x22, 0x33))),
        secondary: Some(AnsiColorEnum::Rgb(RgbColor(0x44, 0x55, 0x66))),
        ..Default::default()
    };
    let light = InlineTheme {
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0xDD, 0xCC, 0xBB))),
        secondary: Some(AnsiColorEnum::Rgb(RgbColor(0xAA, 0x99, 0x88))),
        ..Default::default()
    };
    let input = "/goal @README.md";
    assert_ne!(
        token_span_style(input, dark.clone(), "/goal").fg,
        token_span_style(input, light.clone(), "/goal").fg
    );
    assert_ne!(token_span_style(input, dark, "@README.md").fg, token_span_style(input, light, "@README.md").fg);
}

#[test]
fn prompt_token_colors_fall_back_to_theme_foreground() {
    // No semantic slots set: every token must fall back to the theme foreground
    // instead of a fixed ANSI hue (the old hard-coded yellow/cyan/green).
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        ..Default::default()
    };
    let expected = Some(ratatui_color_from_ansi(theme.foreground.unwrap()));
    let input = "/goal @agent-x @README.md `code`";
    for needle in ["/goal", "@agent-x", "@README.md", "`code`"] {
        assert_eq!(token_span_style(input, theme.clone(), needle).fg, expected, "{needle:?}");
    }
}
