use std::sync::Arc;

use anstyle::{
    Ansi256Color, AnsiColor, Color as AnsiColorEnum, Effects, RgbColor, Style as AnsiStyle,
};
use vtcode_core::ui::theme;
use vtcode_tui::{InlineSegment, InlineTextStyle, convert_style, ui::syntax_highlight};

pub(super) struct PtyLineStyles {
    pub(super) output: Arc<InlineTextStyle>,
    pub(super) glyph: Arc<InlineTextStyle>,
    pub(super) verb: Arc<InlineTextStyle>,
    pub(super) command: Arc<InlineTextStyle>,
    pub(super) args: Arc<InlineTextStyle>,
    pub(super) keyword: Arc<InlineTextStyle>,
    pub(super) variable: Arc<InlineTextStyle>,
    pub(super) string: Arc<InlineTextStyle>,
    pub(super) option: Arc<InlineTextStyle>,
    pub(super) truncation: Arc<InlineTextStyle>,
}

impl PtyLineStyles {
    pub(super) fn new() -> Self {
        let theme_styles = theme::active_styles();
        let output = Arc::new(convert_style(theme_styles.tool_detail.dimmed()));
        let glyph = Arc::new(convert_style(theme_styles.tool_detail.dimmed()));
        let verb = Arc::new(convert_style(
            AnsiStyle::new()
                .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Magenta)))
                .effects(Effects::BOLD),
        ));
        let command = Arc::new(convert_style(
            AnsiStyle::new()
                .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Green)))
                .effects(Effects::BOLD),
        ));
        let args = Arc::new(convert_style(
            AnsiStyle::new()
                .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::White)))
                .effects(Effects::DIMMED),
        ));
        let keyword = Arc::new(convert_style(
            AnsiStyle::new()
                .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Magenta)))
                .effects(Effects::BOLD),
        ));
        let variable = Arc::new(convert_style(
            AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Yellow))),
        ));
        let string = Arc::new(convert_style(
            AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Yellow))),
        ));
        let option = Arc::new(convert_style(
            AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Red))),
        ));
        let truncation = Arc::new(convert_style(theme_styles.tool_detail.dimmed()));

        Self {
            output,
            glyph,
            verb,
            command,
            args,
            keyword,
            variable,
            string,
            option,
            truncation,
        }
    }
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

pub(super) fn tokenize_preserve_whitespace(text: &str) -> Vec<&str> {
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

fn style_for_token<'a>(
    token: &'a str,
    expect_command: &mut bool,
    styles: &'a PtyLineStyles,
) -> Arc<InlineTextStyle> {
    if token.trim().is_empty() {
        return Arc::clone(&styles.output);
    }

    if is_command_separator(token) {
        *expect_command = true;
        return Arc::clone(&styles.args);
    }

    if token.starts_with('"')
        || token.starts_with('\'')
        || token.ends_with('"')
        || token.ends_with('\'')
    {
        *expect_command = false;
        return Arc::clone(&styles.string);
    }

    if token.starts_with('$') || token.contains("=$") || token.starts_with("${") {
        *expect_command = false;
        return Arc::clone(&styles.variable);
    }

    if token.starts_with('-') && token.len() > 1 {
        *expect_command = false;
        return Arc::clone(&styles.option);
    }

    if is_bash_keyword(token) {
        *expect_command = true;
        return Arc::clone(&styles.keyword);
    }

    if *expect_command {
        *expect_command = false;
        return Arc::clone(&styles.command);
    }

    Arc::clone(&styles.args)
}

fn bash_segments(text: &str, styles: &PtyLineStyles, expect_command: bool) -> Vec<InlineSegment> {
    let mut segments = Vec::new();
    let mut command_expected = expect_command;
    for token in tokenize_preserve_whitespace(text) {
        segments.push(InlineSegment {
            text: token.to_string(),
            style: style_for_token(token, &mut command_expected, styles),
        });
    }
    segments
}

fn shell_syntax_segments(
    text: &str,
    styles: &PtyLineStyles,
    expect_command: bool,
) -> Vec<InlineSegment> {
    let semantic = bash_segments(text, styles, expect_command);
    let Some(highlighted) = syntax_highlight::highlight_line_to_anstyle_segments(
        text,
        Some("bash"),
        syntax_highlight::get_active_syntax_theme(),
        true,
    ) else {
        return semantic;
    };

    if highlighted.is_empty() {
        return semantic;
    }

    let converted = highlighted
        .into_iter()
        .map(|(style, text)| InlineSegment {
            text,
            style: Arc::new(convert_style(style).merge_color(styles.args.color)),
        })
        .collect::<Vec<_>>();

    let converted_text = converted
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    if converted_text != text {
        return semantic;
    }

    let non_ws_count = semantic
        .iter()
        .filter(|segment| !segment.text.trim().is_empty())
        .count();
    if non_ws_count > 1 {
        let mut first: Option<&InlineTextStyle> = None;
        let mut has_distinct = false;
        for style in converted
            .iter()
            .filter(|segment| !segment.text.trim().is_empty())
            .map(|segment| segment.style.as_ref())
        {
            if let Some(seed) = first {
                if style != seed {
                    has_distinct = true;
                    break;
                }
            } else {
                first = Some(style);
            }
        }
        if !has_distinct {
            return semantic;
        }
    }

    converted
}

fn ansi_color_from_ansi_code(code: u16) -> Option<AnsiColorEnum> {
    let color = match code {
        30 | 90 => AnsiColor::Black,
        31 | 91 => AnsiColor::Red,
        32 | 92 => AnsiColor::Green,
        33 | 93 => AnsiColor::Yellow,
        34 | 94 => AnsiColor::Blue,
        35 | 95 => AnsiColor::Magenta,
        36 | 96 => AnsiColor::Cyan,
        37 | 97 => AnsiColor::White,
        _ => return None,
    };
    Some(AnsiColorEnum::Ansi(color))
}

fn clear_sgr_effects(effects: &mut Effects, code: u16) {
    match code {
        22 => {
            let _ = effects.remove(Effects::BOLD);
            let _ = effects.remove(Effects::DIMMED);
        }
        23 => {
            let _ = effects.remove(Effects::ITALIC);
        }
        24 => {
            let _ = effects.remove(Effects::UNDERLINE);
        }
        _ => {}
    }
}

fn apply_sgr_codes(sequence: &str, current: &mut InlineTextStyle, fallback: &InlineTextStyle) {
    let params: Vec<u16> = if sequence.trim().is_empty() {
        vec![0]
    } else {
        sequence
            .split(';')
            .map(|value| value.parse::<u16>().unwrap_or(0))
            .collect()
    };

    let mut index = 0usize;
    while index < params.len() {
        let code = params[index];
        match code {
            0 => *current = fallback.clone(),
            1 => current.effects |= Effects::BOLD,
            2 => current.effects |= Effects::DIMMED,
            3 => current.effects |= Effects::ITALIC,
            4 => current.effects |= Effects::UNDERLINE,
            22..=24 => clear_sgr_effects(&mut current.effects, code),
            30..=37 | 90..=97 => current.color = ansi_color_from_ansi_code(code),
            39 => current.color = fallback.color,
            40..=47 | 100..=107 => {
                let fg_code = code - 10;
                current.bg_color = ansi_color_from_ansi_code(fg_code);
            }
            49 => current.bg_color = fallback.bg_color,
            38 | 48 => {
                let is_fg = code == 38;
                if let Some(mode) = params.get(index + 1).copied() {
                    match mode {
                        5 => {
                            if let Some(value) = params.get(index + 2).copied() {
                                let color = AnsiColorEnum::Ansi256(Ansi256Color(value as u8));
                                if is_fg {
                                    current.color = Some(color);
                                } else {
                                    current.bg_color = Some(color);
                                }
                                index += 2;
                            }
                        }
                        2 => {
                            if index + 4 < params.len() {
                                let r = params[index + 2] as u8;
                                let g = params[index + 3] as u8;
                                let b = params[index + 4] as u8;
                                let color = AnsiColorEnum::Rgb(RgbColor(r, g, b));
                                if is_fg {
                                    current.color = Some(color);
                                } else {
                                    current.bg_color = Some(color);
                                }
                                index += 4;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
}

fn sgr_payload(sequence: &str) -> Option<&str> {
    if sequence.starts_with("\u{1b}[") && sequence.ends_with('m') {
        Some(&sequence[2..sequence.len().saturating_sub(1)])
    } else {
        None
    }
}

fn ansi_output_segments(text: &str, styles: &PtyLineStyles) -> Option<Vec<InlineSegment>> {
    if !text.contains('\u{1b}') {
        return None;
    }

    let mut segments = Vec::new();
    let mut current = styles.output.as_ref().clone();
    let fallback = styles.output.as_ref().clone();
    let mut index = 0usize;
    let mut text_buffer = String::new();

    while index < text.len() {
        let Some(remaining) = text.get(index..) else {
            break;
        };
        let Some(first) = remaining.as_bytes().first() else {
            break;
        };

        if *first == 0x1b {
            if !text_buffer.is_empty() {
                segments.push(InlineSegment {
                    text: std::mem::take(&mut text_buffer),
                    style: Arc::new(current.clone()),
                });
            }

            if let Some(len) = vtcode_core::utils::ansi_parser::parse_ansi_sequence(remaining) {
                if let Some(sequence) = remaining.get(..len)
                    && let Some(payload) = sgr_payload(sequence)
                {
                    apply_sgr_codes(payload, &mut current, &fallback);
                }
                index += len;
                continue;
            }

            text_buffer.push_str(remaining);
            index = text.len();
            continue;
        }

        let mut chars = remaining.chars();
        if let Some(ch) = chars.next() {
            text_buffer.push(ch);
            index += ch.len_utf8();
        } else {
            break;
        }
    }

    if !text_buffer.is_empty() {
        segments.push(InlineSegment {
            text: text_buffer,
            style: Arc::new(current),
        });
    }

    if segments.is_empty() {
        return None;
    }
    Some(
        segments
            .into_iter()
            .filter(|segment| !segment.text.is_empty())
            .collect(),
    )
}

fn append_output_segments_with_ansi(
    segments: &mut Vec<InlineSegment>,
    text: &str,
    styles: &PtyLineStyles,
) {
    if let Some(mut ansi_segments) = ansi_output_segments(text, styles) {
        segments.append(&mut ansi_segments);
    } else {
        segments.push(InlineSegment {
            text: text.to_string(),
            style: Arc::clone(&styles.output),
        });
    }
}

pub(super) fn line_to_segments(line: &str, styles: &PtyLineStyles) -> Vec<InlineSegment> {
    if let Some(command_text) = line.strip_prefix("• Ran ") {
        let mut segments = vec![
            InlineSegment {
                text: "• ".to_string(),
                style: Arc::clone(&styles.glyph),
            },
            InlineSegment {
                text: "Ran".to_string(),
                style: Arc::clone(&styles.verb),
            },
            InlineSegment {
                text: " ".to_string(),
                style: Arc::clone(&styles.output),
            },
        ];
        segments.extend(shell_syntax_segments(command_text, styles, true));
        return segments;
    }

    if let Some(text) = line.strip_prefix("  │ ") {
        let mut segments = vec![
            InlineSegment {
                text: "  ".to_string(),
                style: Arc::clone(&styles.output),
            },
            InlineSegment {
                text: "│".to_string(),
                style: Arc::clone(&styles.glyph),
            },
            InlineSegment {
                text: " ".to_string(),
                style: Arc::clone(&styles.output),
            },
        ];
        segments.extend(shell_syntax_segments(text, styles, false));
        return segments;
    }

    if let Some(text) = line.strip_prefix("  └ ") {
        let mut segments = vec![
            InlineSegment {
                text: "  ".to_string(),
                style: Arc::clone(&styles.output),
            },
            InlineSegment {
                text: "└".to_string(),
                style: Arc::clone(&styles.glyph),
            },
            InlineSegment {
                text: " ".to_string(),
                style: Arc::clone(&styles.output),
            },
        ];
        append_output_segments_with_ansi(&mut segments, text, styles);
        return segments;
    }

    if line.trim_start().starts_with('…') {
        return vec![InlineSegment {
            text: line.to_string(),
            style: Arc::clone(&styles.truncation),
        }];
    }

    if let Some(text) = line.strip_prefix("    ") {
        let mut segments = vec![InlineSegment {
            text: "    ".to_string(),
            style: Arc::clone(&styles.output),
        }];
        append_output_segments_with_ansi(&mut segments, text, styles);
        return segments;
    }

    vec![InlineSegment {
        text: line.to_string(),
        style: Arc::clone(&styles.output),
    }]
}
