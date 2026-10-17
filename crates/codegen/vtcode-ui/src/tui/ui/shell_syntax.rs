//! Shell syntax helpers for `• Ran` tool-call lines.
//!
//! Extracted from `src/agent/runloop/unified/tool_pipeline/pty_stream/segments.rs`
//! so both live PTY rendering and the compact-activity row share one
//! tokenizer + palette. Keeps command/args/option/keyword coloring DRY.

use std::sync::Arc;

use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, Style as AnsiStyle};
use vtcode_commons::formatting::{RAN_COMMAND_CONTINUATION_WIDTH, RAN_COMMAND_FIRST_WIDTH};
use vtcode_commons::ui_protocol::{InlineSegment, InlineTextStyle, convert_style};

use crate::tui::ui::syntax_highlight;

pub struct ShellLineStyles {
    pub output: Arc<InlineTextStyle>,
    pub bullet: Arc<InlineTextStyle>,
    pub glyph: Arc<InlineTextStyle>,
    pub verb: Arc<InlineTextStyle>,
    pub command: Arc<InlineTextStyle>,
    pub args: Arc<InlineTextStyle>,
    pub keyword: Arc<InlineTextStyle>,
    pub variable: Arc<InlineTextStyle>,
    pub string: Arc<InlineTextStyle>,
    pub option: Arc<InlineTextStyle>,
    pub truncation: Arc<InlineTextStyle>,
    /// Structural tokens (`|`, `;`, `&&`, redirections) — muted so command
    /// words and args carry the color hierarchy.
    pub separator: Arc<InlineTextStyle>,
    /// Grouped `N commands` counts — bold accent matching the verb so the
    /// collapsed row stays prominent instead of washing out.
    pub count: Arc<InlineTextStyle>,
}

impl ShellLineStyles {
    /// Styles derived from the process-global UI theme (used by PTY live view
    /// when no session is available). The binary's PTY stream reuses this type.
    pub fn new() -> Self {
        let theme_styles = crate::theme::active_styles();
        Self::from_ansi_styles(theme_styles.primary, theme_styles.pty_output)
    }

    /// Styles derived from a session's resolved theme — preferred inside
    /// `AppSession`/`Session` where `InlineTheme` is already available.
    pub fn from_session(_session: &crate::tui::core_tui::app::session::AppSession) -> Self {
        let theme_styles = crate::theme::active_styles();
        // Keep verb synced with the session's primary, body with pty_output
        // so compact rows track theme changes (e.g. catppuccin-latte).
        Self::from_ansi_styles(theme_styles.primary, theme_styles.pty_output)
    }

    fn from_ansi_styles(primary: AnsiStyle, pty_output: AnsiStyle) -> Self {
        let output = Arc::new(convert_style(pty_output));
        let magenta_bold = Arc::new(convert_style(
            AnsiStyle::new()
                .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Magenta)))
                .effects(Effects::BOLD),
        ));
        let accent_bold = Arc::new(convert_style(primary | Effects::BOLD));
        let yellow = Arc::new(convert_style(AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Yellow)))));
        // Args use the themed body color (opaque) instead of hardcoded dimmed
        // white so paths/args stay legible on light themes; separators stay
        // dimmed so command words keep the visual hierarchy.
        let args = Arc::new(convert_style(pty_output));

        Self {
            output: Arc::clone(&output),
            bullet: Arc::new(convert_style(AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Green))))),
            glyph: Arc::clone(&output),
            verb: accent_bold,
            command: Arc::new(convert_style(
                AnsiStyle::new()
                    .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Green)))
                    .effects(Effects::BOLD),
            )),
            args: Arc::clone(&args),
            keyword: magenta_bold,
            variable: Arc::clone(&yellow),
            string: yellow,
            option: Arc::new(convert_style(AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Red))))),
            truncation: Arc::new(convert_style(pty_output | Effects::DIMMED)),
            separator: Arc::new(convert_style(pty_output | Effects::DIMMED)),
            count: Arc::new(convert_style(primary | Effects::BOLD)),
        }
    }
}

impl Default for ShellLineStyles {
    fn default() -> Self {
        Self::new()
    }
}

pub fn is_bash_keyword(token: &str) -> bool {
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

pub fn is_command_separator(token: &str) -> bool {
    matches!(token, "|" | "||" | "&&" | ";" | ";;" | "&")
}

/// Redirection operators, including fd-prefixed and target-attached forms:
/// `>`, `>>`, `2>`, `2>&1`, `2>/dev/null`, `<`, `<<`.
fn is_redirection_token(token: &str) -> bool {
    let body = token.trim_start_matches(|c: char| c.is_ascii_digit());
    body.starts_with('>') || body.starts_with('<')
}

pub fn tokenize_preserve_whitespace(text: &str) -> Vec<&str> {
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

fn style_for_token<'a>(token: &'a str, expect_command: &mut bool, styles: &'a ShellLineStyles) -> Arc<InlineTextStyle> {
    if token.trim().is_empty() {
        return Arc::clone(&styles.output);
    }

    if is_command_separator(token) {
        *expect_command = true;
        return Arc::clone(&styles.separator);
    }

    if is_redirection_token(token) {
        *expect_command = false;
        return Arc::clone(&styles.separator);
    }

    if token.starts_with('"') || token.starts_with('\'') || token.ends_with('"') || token.ends_with('\'') {
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

/// Split trailing `;`/`&`/`|` runs off a whitespace-delimited token so
/// attached separators (`-120;`, `'---';`) color as separators instead of
/// inheriting the word's option/string style. Redirections (`2>/dev/null`,
/// `2>&1`) end in word characters and are left intact.
fn split_trailing_command_operators(token: &str) -> Vec<&str> {
    let bytes = token.as_bytes();
    let mut end = bytes.len();
    while end > 0 && matches!(bytes[end - 1], b';' | b'&' | b'|') {
        end -= 1;
    }
    if end == 0 || end == bytes.len() {
        return vec![token];
    }
    vec![&token[..end], &token[end..]]
}

fn bash_segments(text: &str, styles: &ShellLineStyles, expect_command: bool) -> Vec<InlineSegment> {
    let mut segments = Vec::new();
    let mut command_expected = expect_command;
    for token in tokenize_preserve_whitespace(text) {
        if token.trim().is_empty() {
            segments.push(InlineSegment {
                text: token.to_string(),
                style: Arc::clone(&styles.output),
            });
            continue;
        }
        for part in split_trailing_command_operators(token) {
            segments.push(InlineSegment {
                text: part.to_string(),
                style: style_for_token(part, &mut command_expected, styles),
            });
        }
    }
    segments
}

pub fn shell_syntax_segments(text: &str, styles: &ShellLineStyles, expect_command: bool) -> Vec<InlineSegment> {
    let highlighted = syntax_highlight::highlight_line_to_anstyle_segments(
        text,
        Some("bash"),
        syntax_highlight::get_active_syntax_theme(),
        true,
    );
    shell_syntax_segments_with_highlighted(text, styles, expect_command, highlighted)
}

/// Prefer shell syntax highlighting only when it keeps token colors distinct.
///
/// A handful of syntax themes return one foreground color for a shell line.
/// In that case the semantic tokenizer provides more useful command, option,
/// and argument styling than accepting the uniform highlighter result.
pub(crate) fn shell_syntax_segments_with_highlighted(
    text: &str,
    styles: &ShellLineStyles,
    expect_command: bool,
    highlighted: Option<Vec<(AnsiStyle, String)>>,
) -> Vec<InlineSegment> {
    let semantic = bash_segments(text, styles, expect_command);
    let Some(highlighted) = highlighted else {
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

    let converted_text = converted.iter().map(|segment| segment.text.as_str()).collect::<String>();
    if converted_text != text {
        return semantic;
    }

    let non_ws_count = semantic.iter().filter(|segment| !segment.text.trim().is_empty()).count();
    if non_ws_count > 1 {
        let mut first_colors: Option<(Option<AnsiColorEnum>, Option<AnsiColorEnum>)> = None;
        let mut has_distinct = false;
        for style in converted
            .iter()
            .filter(|segment| !segment.text.trim().is_empty())
            .map(|segment| segment.style.as_ref())
        {
            let colors = (style.color, style.bg_color);
            if let Some(seed) = first_colors {
                if colors != seed {
                    has_distinct = true;
                    break;
                }
            } else {
                first_colors = Some(colors);
            }
        }
        if !has_distinct {
            return semantic;
        }
    }

    converted
}

pub fn line_to_compact_segments(
    metadata: &vtcode_commons::ui_protocol::CompactActivityMetadata,
    styles: &ShellLineStyles,
) -> Vec<InlineSegment> {
    // • Ran <command>  (single) or • Ran N commands (grouped)
    let mut segments = Vec::new();
    segments.push(InlineSegment {
        text: "• ".to_string(),
        style: Arc::clone(&styles.bullet),
    });
    segments.push(InlineSegment {
        text: "Ran".to_string(),
        style: Arc::clone(&styles.verb),
    });
    segments.push(InlineSegment {
        text: " ".to_string(),
        style: Arc::clone(&styles.output),
    });

    if metadata.command_count > 1 {
        // Grouped: bold accent count matches the verb so `Ran 4 commands`
        // stays prominent instead of washing out.
        segments.push(InlineSegment {
            text: format!("{} commands", metadata.command_count),
            style: Arc::clone(&styles.count),
        });
    } else if let Some(cmd) = metadata.command.as_deref() {
        // Long single-command rows wrap with explicit `\` continuations so a
        // chained `git add … && git commit … && git log …` reads as one shell
        // command instead of terminal-reflow word wrap. Short rows stay
        // single-line; `\n` splits are honored by transcript reflow.
        const COMPACT_CONT_INDENT: &str = "\n      ";
        let needs_wrap = cmd.chars().count() > RAN_COMMAND_FIRST_WIDTH;
        if needs_wrap {
            // Operator-aware (same `&&`/`||`/`|` chunking as the expanded
            // `• Ran` headers): each chain/pipe stage starts on a fresh row.
            // The wrapper already appends ` \\` to non-final rows; strip it
            // here so the marker is emitted once with separator styling.
            let wrapped = vtcode_commons::formatting::wrap_shell_command_with_continuations(
                cmd,
                RAN_COMMAND_FIRST_WIDTH,
                RAN_COMMAND_CONTINUATION_WIDTH,
            );
            if wrapped.len() > 1 {
                for (idx, line) in wrapped.iter().enumerate() {
                    // First wrapped line uses command-position highlighting;
                    // continuations highlight as args (no leading command word).
                    let expect_command = idx == 0;
                    let is_final_line = idx + 1 == wrapped.len();
                    // The wrapper appends ` \` to non-final rows only, so on
                    // the final row a trailing ` \` is the command's own text
                    // (a real line continuation) and must be kept verbatim.
                    let body = if is_final_line {
                        line.as_str()
                    } else {
                        line.strip_suffix(" \\").unwrap_or(line)
                    };
                    segments.extend(shell_syntax_segments(body, styles, expect_command));
                    if idx + 1 < wrapped.len() {
                        segments.push(InlineSegment {
                            text: " \\".to_string(),
                            style: Arc::clone(&styles.separator),
                        });
                        segments.push(InlineSegment {
                            text: COMPACT_CONT_INDENT.to_string(),
                            style: Arc::clone(&styles.output),
                        });
                    }
                }
            } else {
                segments.extend(shell_syntax_segments(cmd, styles, true));
            }
        } else {
            segments.extend(shell_syntax_segments(cmd, styles, true));
        }
        if metadata.hidden_line_count > 0 {
            segments.push(InlineSegment {
                text: format!(" · … +{} lines", metadata.hidden_line_count),
                style: Arc::clone(&styles.truncation),
            });
        }
    } else {
        segments.push(InlineSegment {
            text: "command".to_string(),
            style: Arc::clone(&styles.args),
        });
    }

    if let Some(suffix) = metadata.suffix.as_deref().filter(|s| !s.is_empty()) {
        segments.push(InlineSegment {
            text: " · ".to_string(),
            style: Arc::clone(&styles.truncation),
        });
        segments.push(InlineSegment {
            text: suffix.to_string(),
            style: Arc::clone(&styles.truncation),
        });
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_header_preserves_distinct_semantic_token_colors() {
        let styles = ShellLineStyles::new();
        let segments =
            shell_syntax_segments("find src/agent/runloop -maxdepth 3 -type f -name *.rs | sort", &styles, true);
        let option = segments
            .iter()
            .find(|segment| segment.text.contains("maxdepth"))
            .expect("option token");
        let command = segments
            .iter()
            .find(|segment| segment.text.contains("find"))
            .expect("command token");
        assert_ne!(option.style.color, command.style.color);
    }

    #[test]
    fn single_command_row_carries_full_pipeline_without_truncation() {
        // Screenshot 2026-09-24 16:37: a single-command `• Ran` compact row
        // must carry the complete pipeline — `||` inside the quoted pattern,
        // every `| grep -v` segment, and the `\.backup` arg — with no `…`.
        // (Viewport-aware wrapping owns the overflow at render time.)
        let styles = ShellLineStyles::new();
        let command = "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs | grep -v node_modules | grep -v package-lock | grep -v \"\\.backup\"";
        let meta = vtcode_commons::ui_protocol::CompactActivityMetadata {
            group_id: 1,
            command_count: 1,
            command: Some(command.into()),
            hidden_line_count: 0,
            suffix: None,
            review_anchor: None,
            review_anchors: vec![],
        };
        let segs = line_to_compact_segments(&meta, &styles);
        let text: String = segs.iter().map(|s| s.text.as_str()).collect();
        assert!(!text.contains('…'), "compact row must not truncate, got: {text:?}");
        assert!(text.starts_with("• Ran "), "got: {text:?}");
        assert_eq!(text.matches('|').count(), 6, "pattern pipes + shell pipes must survive: {text:?}");
        for fragment in ["node_modules", "package-lock", "\\.backup"] {
            assert!(text.contains(fragment), "missing {fragment:?} in {text:?}");
        }
    }

    #[test]
    fn compact_row_keeps_command_own_trailing_continuation_backslash() {
        // The wrapper appends ` \` to non-final rows only, so on the final
        // wrapped row a trailing ` \` belongs to the command itself (a real
        // line continuation). Stripping it there would make the transcript
        // diverge from what executed.
        let styles = ShellLineStyles::new();
        let with_continuation = "echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \\";
        assert!(with_continuation.chars().count() > RAN_COMMAND_FIRST_WIDTH);
        let meta = vtcode_commons::ui_protocol::CompactActivityMetadata {
            group_id: 1,
            command_count: 1,
            command: Some(with_continuation.into()),
            hidden_line_count: 0,
            suffix: None,
            review_anchor: None,
            review_anchors: vec![],
        };
        let text: String = line_to_compact_segments(&meta, &styles)
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert!(text.ends_with(" \\"), "command's own trailing continuation must survive: {text:?}");

        // Asymmetric check: a command that does not end in ` \` gains no
        // marker on the final row either.
        let without_continuation = "echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa target";
        let meta = vtcode_commons::ui_protocol::CompactActivityMetadata {
            group_id: 1,
            command_count: 1,
            command: Some(without_continuation.into()),
            hidden_line_count: 0,
            suffix: None,
            review_anchor: None,
            review_anchors: vec![],
        };
        let text: String = line_to_compact_segments(&meta, &styles)
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert!(!text.ends_with(" \\"), "no continuation marker may appear on the final row: {text:?}");
    }

    #[test]
    fn grouped_has_no_single_command_highlight() {
        let styles = ShellLineStyles::new();
        let meta = vtcode_commons::ui_protocol::CompactActivityMetadata {
            group_id: 1,
            command_count: 4,
            command: None,
            hidden_line_count: 10,
            suffix: Some("output retained".into()),
            review_anchor: Some(1),
            review_anchors: vec![1],
        };
        let segs = line_to_compact_segments(&meta, &styles);
        let text: String = segs.iter().map(|s| s.text.as_str()).collect();
        assert!(text.contains("4 commands"));
        assert!(text.contains("output retained"));
    }

    #[test]
    fn grouped_count_stays_bold_instead_of_dimmed() {
        let styles = ShellLineStyles::new();
        assert!(styles.count.effects.contains(Effects::BOLD));
        assert!(!styles.count.effects.contains(Effects::DIMMED));
    }

    #[test]
    fn args_use_themed_body_color_for_light_theme_legibility() {
        let styles = ShellLineStyles::new();
        assert_eq!(styles.args.color, styles.output.color);
    }

    #[test]
    fn attached_separator_splits_from_word_style() {
        let styles = ShellLineStyles::new();
        let segments = bash_segments("head -120; echo hi", &styles, true);
        let option = segments.iter().find(|s| s.text == "-120").expect("option part");
        let separator = segments.iter().find(|s| s.text == ";").expect("separator part");
        assert_eq!(option.style.color, styles.option.color);
        assert_eq!(separator.style.color, styles.separator.color);
        assert!(separator.style.effects.contains(Effects::DIMMED));
    }

    #[test]
    fn uniform_syntax_highlighting_uses_semantic_fallback() {
        let styles = ShellLineStyles::new();
        let highlighted_style = AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Cyan)));
        let highlighted = ["cargo", " ", "check", " ", "-p", " ", "vtcode"]
            .into_iter()
            .map(|text| (highlighted_style, text.to_string()))
            .collect();

        let segments =
            shell_syntax_segments_with_highlighted("cargo check -p vtcode", &styles, true, Some(highlighted));
        let command = segments.iter().find(|segment| segment.text == "cargo").expect("command token");
        let option = segments.iter().find(|segment| segment.text == "-p").expect("option token");

        assert_eq!(command.style.color, styles.command.color);
        assert_eq!(option.style.color, styles.option.color);
        assert_ne!(command.style.color, option.style.color);
    }
}
