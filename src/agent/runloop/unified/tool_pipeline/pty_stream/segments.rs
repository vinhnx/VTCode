use std::sync::Arc;

use anstyle::{Ansi256Color, AnsiColor, Color as AnsiColorEnum, Effects, RgbColor};
use vtcode_ui::tui::app::{InlineLinkRange, InlineLinkTarget, InlineSegment, InlineTextStyle};
pub(super) use vtcode_ui::tui::ui::shell_syntax::ShellLineStyles as PtyLineStyles;
use vtcode_ui::tui::ui::shell_syntax::shell_syntax_segments;

fn glyph_prefix(glyph: char, styles: &PtyLineStyles) -> Vec<InlineSegment> {
    vec![
        InlineSegment {
            text: "  ".to_string(),
            style: Arc::clone(&styles.output),
        },
        InlineSegment {
            text: glyph.to_string(),
            style: Arc::clone(&styles.glyph),
        },
        InlineSegment {
            text: " ".to_string(),
            style: Arc::clone(&styles.output),
        },
    ]
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
        sequence.split(';').map(|value| value.parse::<u16>().unwrap_or(0)).collect()
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
                        2 if index + 4 < params.len() => {
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

fn parse_osc8_target(sequence: &str) -> Option<Option<String>> {
    let payload = sequence.strip_prefix("\u{1b}]8;")?;
    let payload = payload.strip_suffix("\u{1b}\\").or_else(|| payload.strip_suffix('\u{7}'))?;
    let (_, uri) = payload.split_once(';')?;
    if uri.is_empty() {
        Some(None)
    } else {
        Some(Some(uri.to_string()))
    }
}

fn ansi_output_segments(text: &str, styles: &PtyLineStyles) -> Option<(Vec<InlineSegment>, Vec<InlineLinkRange>)> {
    if !text.contains('\u{1b}') {
        return None;
    }

    let mut segments = Vec::new();
    let mut link_ranges = Vec::new();
    let mut current = styles.output.as_ref().clone();
    let fallback = styles.output.as_ref().clone();
    let mut active_link: Option<String> = None;
    let mut visible_offset = 0usize;
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
                let text = std::mem::take(&mut text_buffer);
                let end = visible_offset + text.len();
                if let Some(url) = active_link.clone() {
                    link_ranges.push(InlineLinkRange {
                        start: visible_offset,
                        end,
                        target: InlineLinkTarget::Url(url),
                    });
                }
                segments.push(InlineSegment { text, style: Arc::new(current.clone()) });
                visible_offset = end;
            }

            if let Some(len) = vtcode_core::utils::ansi_parser::parse_ansi_sequence(remaining) {
                if let Some(sequence) = remaining.get(..len) {
                    if let Some(payload) = sgr_payload(sequence) {
                        apply_sgr_codes(payload, &mut current, &fallback);
                    } else if let Some(target) = parse_osc8_target(sequence) {
                        active_link = target;
                    }
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
        let end = visible_offset + text_buffer.len();
        if let Some(url) = active_link {
            link_ranges.push(InlineLinkRange {
                start: visible_offset,
                end,
                target: InlineLinkTarget::Url(url),
            });
        }
        segments.push(InlineSegment { text: text_buffer, style: Arc::new(current) });
    }

    if segments.is_empty() {
        return None;
    }
    Some((segments.into_iter().filter(|segment| !segment.text.is_empty()).collect(), link_ranges))
}

fn append_output_segments_with_ansi(
    segments: &mut Vec<InlineSegment>,
    link_ranges: &mut Vec<InlineLinkRange>,
    text: &str,
    styles: &PtyLineStyles,
) {
    if let Some((mut ansi_segments, ansi_links)) = ansi_output_segments(text, styles) {
        segments.append(&mut ansi_segments);
        link_ranges.extend(ansi_links);
    } else {
        segments.push(InlineSegment {
            text: text.to_string(),
            style: Arc::clone(&styles.output),
        });
    }
}

fn split_shell_continuation(text: &str) -> (&str, bool) {
    // `wrap_shell_command_with_continuations` marks every wrapped line except
    // the last with a trailing ` \`. Strip it before shell highlighting so the
    // marker never confuses the bash grammar, then re-emit it dimmed.
    if let Some(stripped) = text.strip_suffix(" \\") {
        (stripped, true)
    } else {
        (text, false)
    }
}

fn continuation_marker_segment(styles: &PtyLineStyles) -> InlineSegment {
    let mut style = styles.glyph.as_ref().clone();
    style.effects |= Effects::DIMMED;
    InlineSegment { text: " \\".to_string(), style: Arc::new(style) }
}

pub(super) fn line_to_segments(line: &str, styles: &PtyLineStyles) -> (Vec<InlineSegment>, Vec<InlineLinkRange>) {
    if let Some(command_text) = line.strip_prefix("• Ran ") {
        let mut segments = vec![
            InlineSegment {
                text: "• ".to_string(),
                style: Arc::clone(&styles.bullet),
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
        let (body, has_continuation) = split_shell_continuation(command_text);
        segments.extend(shell_syntax_segments(body, styles, true));
        if has_continuation {
            segments.push(continuation_marker_segment(styles));
        }
        return (segments, Vec::new());
    }

    if let Some(text) = line.strip_prefix("  │ ") {
        let mut segments = glyph_prefix('│', styles);
        let (body, has_continuation) = split_shell_continuation(text);
        segments.extend(shell_syntax_segments(body, styles, false));
        if has_continuation {
            segments.push(continuation_marker_segment(styles));
        }
        return (segments, Vec::new());
    }

    if let Some(text) = line.strip_prefix("  └ ") {
        let mut segments = glyph_prefix('└', styles);
        let mut link_ranges = Vec::new();
        append_output_segments_with_ansi(&mut segments, &mut link_ranges, text, styles);
        return (segments, shift_link_ranges(&link_ranges, 4));
    }

    if line.trim_start().starts_with('…') {
        return (
            vec![InlineSegment {
                text: line.to_string(),
                style: Arc::clone(&styles.truncation),
            }],
            Vec::new(),
        );
    }

    if let Some(text) = line.strip_prefix("    ") {
        let mut segments = vec![InlineSegment {
            text: "    ".to_string(),
            style: Arc::clone(&styles.output),
        }];
        let mut link_ranges = Vec::new();
        append_output_segments_with_ansi(&mut segments, &mut link_ranges, text, styles);
        return (segments, shift_link_ranges(&link_ranges, 4));
    }

    (
        vec![InlineSegment {
            text: line.to_string(),
            style: Arc::clone(&styles.output),
        }],
        Vec::new(),
    )
}

fn shift_link_ranges(ranges: &[InlineLinkRange], by: usize) -> Vec<InlineLinkRange> {
    ranges
        .iter()
        .cloned()
        .map(|mut range| {
            range.start += by;
            range.end += by;
            range
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_output_extracts_osc8_hyperlinks() {
        let styles = PtyLineStyles::new();
        let (segments, link_ranges) =
            line_to_segments("  └ Go \u{1b}]8;;https://example.com/docs\u{1b}\\docs\u{1b}]8;;\u{1b}\\ now", &styles);

        let text = segments.iter().map(|segment| segment.text.as_str()).collect::<String>();
        assert_eq!(text, "  └ Go docs now");
        assert_eq!(link_ranges.len(), 1);
        assert_eq!(link_ranges[0].start, 7);
        assert_eq!(link_ranges[0].end, 11);
        assert!(matches!(
            &link_ranges[0].target,
            InlineLinkTarget::Url(url) if url == "https://example.com/docs"
        ));
    }

    #[test]
    fn ran_header_continuation_marker_is_stripped_from_highlighting_and_reemitted_dimmed() {
        let styles = PtyLineStyles::new();
        let (segments, links) = line_to_segments("• Ran echo a \\", &styles);
        assert!(links.is_empty());
        let text: String = segments.iter().map(|segment| segment.text.as_str()).collect();
        assert_eq!(text, "• Ran echo a \\");
        let marker = segments.last().expect("continuation marker segment");
        assert_eq!(marker.text, " \\");
        assert!(marker.style.effects.contains(Effects::DIMMED));
        // The marker must not leak into the highlighted body: no body segment
        // other than the marker itself ends with a backslash.
        for segment in &segments[..segments.len() - 1] {
            assert!(!segment.text.ends_with('\\'), "marker leaked into body: {segment:?}");
        }
    }

    #[test]
    fn ran_continuation_row_without_marker_has_no_trailing_segment() {
        let styles = PtyLineStyles::new();
        let (plain, _) = line_to_segments("  │ git status --short", &styles);
        assert!(!plain.iter().any(|segment| segment.text == " \\"));
        let (marked, _) = line_to_segments("  │ git add a \\", &styles);
        let text: String = marked.iter().map(|segment| segment.text.as_str()).collect();
        assert_eq!(text, "  │ git add a \\");
        assert_eq!(marked.last().map(|segment| segment.text.as_str()), Some(" \\"));
    }

    #[test]
    fn command_header_preserves_distinct_semantic_token_colors() {
        let styles = PtyLineStyles::new();
        let (segments, _) =
            line_to_segments("• Ran find src/agent/runloop -maxdepth 3 -type f -name *.rs | sort", &styles);

        let option = segments
            .iter()
            .find(|segment| segment.text.contains("maxdepth"))
            .unwrap_or_else(|| {
                panic!("expected option token in segments: {segments:?}");
            });
        let command = segments
            .iter()
            .find(|segment| segment.text.contains("find"))
            .unwrap_or_else(|| panic!("expected command token in segments: {segments:?}"));
        assert_ne!(option.style.color, command.style.color);
    }
}
