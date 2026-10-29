use super::*;

#[test]
fn pty_wrapped_lines_do_not_exceed_viewport_width() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_pty_line(&mut session, "  └ this PTY output line wraps on narrow widths");

    let width = 18usize;
    let rendered = session.reflow_pty_lines(0, width as u16);
    for line in rendered {
        let line_width: usize = line.line.spans.iter().map(|span| span.width()).sum();
        assert!(line_width <= width, "wrapped PTY line exceeded viewport width: {line_width} > {width}",);
    }
}
#[test]
fn pty_command_header_wraps_in_full_without_truncation_at_narrow_width() {
    // Screenshot 2026-09-24 16:37 end to end: the shell-aware pre-wrap
    // (`wrap_shell_command` at 62/58) emits three logical header lines, and
    // viewport reflow must keep every pipe segment with no `…` at any width.
    // Wide viewports preserve the logical rows (and their quote-atomic
    // breaks) 1:1; narrow viewports may re-break mid-quote to fit, but must
    // stay lossless and within bounds.
    let command = "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs | grep -v node_modules | grep -v package-lock | grep -v \"\\.backup\"";
    let logical = vtcode_commons::formatting::wrap_shell_command(command, 62, 58);
    assert_eq!(logical.len(), 3, "fixture must span three header lines: {logical:?}");

    let push_header = |session: &mut Session| {
        push_pty_line(session, &format!("• Ran {}", logical[0]));
        for segment in logical.iter().skip(1) {
            push_pty_line(session, &format!("  │ {segment}"));
        }
    };
    let reflow_all = |session: &Session, width: u16| {
        let mut rows = Vec::new();
        for index in 0..session.lines.len() {
            rows.extend(session.reflow_pty_lines(index, width));
        }
        rows
    };

    // Wide: logical rows survive 1:1 with the quoted pattern intact.
    let mut wide = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_header(&mut wide);
    let wide_rows = reflow_all(&wide, 80);
    let wide_texts: Vec<String> = wide_rows
        .iter()
        .map(|line| line_text(&line.line))
        .filter(|text| !text.trim().is_empty())
        .collect();
    assert_eq!(
        wide_texts,
        vec![
            format!("• Ran {}", logical[0]),
            format!("  │ {}", logical[1]),
            format!("  │ {}", logical[2]),
        ],
        "wide viewport must preserve the logical header rows"
    );
    let wide_joined = wide_texts.join("\n");
    assert!(!wide_joined.contains('…'), "wide reflow must not truncate: {wide_joined:?}");
    assert!(
        wide_joined.contains("\"@vinhnx/vtcode|npm install -g||npx @vinhnx\""),
        "quoted pattern lost: {wide_joined:?}"
    );
    assert_eq!(wide_joined.matches('|').count(), 6, "pattern + shell pipes must survive: {wide_joined:?}");

    // Narrow: re-breaks are allowed, but nothing may be lost or overflow.
    let mut narrow = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_header(&mut narrow);
    let narrow_rows = reflow_all(&narrow, 40);
    let narrow_texts: Vec<String> = narrow_rows.iter().map(|line| line_text(&line.line)).collect();
    let narrow_joined = narrow_texts.join("\n");
    assert!(!narrow_joined.contains('…'), "narrow reflow must not truncate: {narrow_joined:?}");
    let flat: String = narrow_joined.chars().filter(|c| !c.is_whitespace()).collect();
    let expected_flat: String = wide_texts.join("").chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(flat, expected_flat, "narrow reflow lost content: {narrow_joined:?}");
    // Substring checks run on the whitespace-stripped text because narrow
    // viewports may re-break long tokens across rows (e.g. `p`/`ackage-lock`).
    assert!(flat.contains("package-lock"), "pipe segment lost: {narrow_joined:?}");
    assert!(flat.contains("\"\\.backup\""), "final pipe arg lost: {narrow_joined:?}");
    assert!(narrow_joined.contains('│'), "continuation glyph lost: {narrow_joined:?}");
    for line in &narrow_rows {
        let row_width: usize = line.line.spans.iter().map(|span| span.width()).sum();
        assert!(
            row_width <= 40,
            "reflowed header row exceeded viewport width: {row_width} > 40: {:?}",
            line_text(&line.line)
        );
    }
}
#[test]
fn tool_diff_numbered_lines_keep_hanging_indent_when_wrapped() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text: "459 + let digits_len = digits.chars().take_while(|c| c.is_ascii_digit()).count();".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let rendered = session.reflow_transcript_lines(40);
    let content_lines: Vec<String> = rendered.iter().map(line_text).filter(|text| !text.is_empty()).collect();
    assert!(
        content_lines.len() >= 2,
        "expected wrapped tool diff output, got {} content line(s)",
        content_lines.len()
    );

    let first = &content_lines[0];
    let second = &content_lines[1];

    assert!(first.contains("459 + "), "first line should include diff gutter: {first:?}");
    assert!(
        second.starts_with("          "),
        "wrapped line should keep hanging indent after tool prefix, got: {second:?}"
    );
}
#[test]
fn compact_tinted_diff_rows_keep_hanging_indent_when_wrapped() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    // Compact layout: one leading marker cell, no +/- gutter or line numbers.
    let body = format!(" | {} | {} |", "What can go wrong", "How VT Code responds ".repeat(6));
    let style = InlineTextStyle {
        color: Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack)),
        bg_color: Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green)),
        ..InlineTextStyle::default()
    };
    session.push_line(InlineMessageKind::Tool, vec![InlineSegment { text: body.clone(), style: Arc::new(style) }]);

    let rendered = session.reflow_transcript_lines(40);
    let content_lines: Vec<String> = rendered.iter().map(line_text).filter(|text| !text.trim().is_empty()).collect();
    assert!(content_lines.len() >= 2, "expected wrapped compact diff row: {content_lines:?}");
    assert!(content_lines[0].contains("What can go wrong"));
    // Tool blocks already pad continuations; require hang depth for the
    // compact marker cell plus tool prefix, not a vacuous single space.
    assert!(
        content_lines[1].starts_with("  ") && !content_lines[1].starts_with("   |"),
        "compact continuation should hang under the marker/tool prefix, got: {:?}",
        content_lines[1]
    );
}
#[test]
fn agent_numbered_code_lines_keep_hanging_indent_when_wrapped() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Agent,
        vec![
            InlineSegment {
                text: " 12  ".to_string(),
                style: Arc::new(InlineTextStyle {
                    effects: anstyle::Effects::DIMMED,
                    ..InlineTextStyle::default()
                }),
            },
            make_segment("fn wrapped_diff_continuation_prefix(line_text: &str) -> Option<String> {"),
        ],
    );

    let rendered = session.reflow_transcript_lines(36);
    let content_lines: Vec<String> = rendered.iter().map(line_text).filter(|text| !text.trim().is_empty()).collect();
    assert!(content_lines.len() >= 2, "expected wrapped code line, got: {content_lines:?}");

    let first = &content_lines[0];
    let second = &content_lines[1];
    // Zero-indent: only the code gutter hangs on continuation rows.
    let expected_prefix = " ".repeat(" 12  ".chars().count());

    assert!(first.contains("12  fn wrapped_diff"), "first line was: {first:?}");
    assert!(
        second.starts_with(&expected_prefix),
        "wrapped code continuation should keep gutter indent, got: {second:?}"
    );
}
#[test]
fn agent_omitted_code_lines_keep_hanging_indent_when_wrapped() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "21-421  … [+400 lines omitted; use read_file with offset/limit (1-indexed line numbers) for full content]".to_string(),
            style: Arc::new(InlineTextStyle {
                effects: anstyle::Effects::DIMMED,
                ..InlineTextStyle::default()
            }),
        }],
    );

    let rendered = session.reflow_transcript_lines(52);
    let content_lines: Vec<String> = rendered.iter().map(line_text).filter(|text| !text.trim().is_empty()).collect();
    assert!(content_lines.len() >= 2, "expected wrapped omitted line, got: {content_lines:?}");

    let first = &content_lines[0];
    let second = &content_lines[1];
    // Zero-indent: only the code gutter hangs on continuation rows.
    let expected_prefix = " ".repeat("21-421  ".chars().count());

    assert!(first.contains("21-421"), "first line was: {first:?}");
    assert!(first.contains("…"), "first line was: {first:?}");
    assert!(first.contains("[+400"), "first line was: {first:?}");
    assert!(
        second.starts_with(&expected_prefix),
        "wrapped omitted-line continuation should keep gutter indent, got: {second:?} expected: {expected_prefix:?}"
    );
}
#[test]
fn pty_command_header_verb_uses_primary_color_bullet_uses_theme_foreground() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Pty,
        vec![
            InlineSegment {
                text: "• Ran".to_string(),
                style: Arc::new(InlineTextStyle::default()),
            },
            InlineSegment {
                text: " cat file".to_string(),
                style: Arc::new(InlineTextStyle::default()),
            },
        ],
    );

    let rendered = session.reflow_pty_lines(0, 80);
    let spans: Vec<_> = rendered.iter().flat_map(|line| line.line.spans.iter()).collect();

    // Bullet "• " → theme foreground, no bold
    let bullet_span = spans.iter().find(|s| s.content.as_ref() == "• ").expect("expected • span");
    assert!(
        !bullet_span.style.add_modifier.contains(Modifier::BOLD),
        "• bullet should NOT be bold, got modifiers: {:?}",
        bullet_span.style.add_modifier,
    );

    let theme_fg = InlineTheme::default().foreground.map(ratatui_color_from_ansi);
    assert_eq!(bullet_span.style.fg, theme_fg, "bullet fg should be theme foreground");

    // Verb "Ran" → primary/neutral header color + bold
    let verb_span = spans.iter().find(|s| s.content.as_ref() == "Ran").expect("expected verb span");
    assert!(
        verb_span.style.add_modifier.contains(Modifier::BOLD),
        "verb should be bold, got modifiers: {:?}",
        verb_span.style.add_modifier,
    );
    let theme_primary = InlineTheme::default()
        .primary
        .or(InlineTheme::default().foreground)
        .map(ratatui_color_from_ansi);
    assert_eq!(verb_span.style.fg, theme_primary, "Ran verb should use the header primary color");
}
#[test]
fn pty_command_header_removes_dim_from_all_header_spans() {
    let foreground = AnsiColorEnum::Rgb(RgbColor(0xCC, 0xCC, 0xCC));
    let subdued = AnsiColorEnum::Rgb(RgbColor(0x66, 0x66, 0x66));
    let dimmed = Arc::new(InlineTextStyle {
        color: Some(subdued),
        effects: anstyle::Effects::DIMMED,
        ..InlineTextStyle::default()
    });
    let mut session = Session::new(
        InlineTheme {
            foreground: Some(foreground),
            pty_body: Some(subdued),
            tool_body: Some(subdued),
            ..Default::default()
        },
        None,
        VIEW_ROWS,
    );
    session.push_line(
        InlineMessageKind::Pty,
        vec![
            InlineSegment {
                text: "• ".to_string(), style: Arc::clone(&dimmed)
            },
            InlineSegment {
                text: "Ran".to_string(),
                style: Arc::clone(&dimmed),
            },
            InlineSegment { text: " sed -n 1,260p".to_string(), style: dimmed },
        ],
    );

    let rendered = session.reflow_pty_lines(0, 80);
    let command_span = rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("sed"))
        .expect("expected command header span");

    assert!(!command_span.style.add_modifier.contains(Modifier::DIM));
    assert_eq!(command_span.style.fg, Some(Color::Rgb(0xCC, 0xCC, 0xCC)));
}
#[test]
fn pty_command_headers_remain_opaque_after_prior_output() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "• Ran first-command".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "    first output".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "• Ran second-command".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let rendered = session.reflow_pty_lines(2, 80);
    let second_header = rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("second-command"))
        .expect("expected second command header");

    assert!(!second_header.style.add_modifier.contains(Modifier::DIM));
}
#[test]
fn pty_command_header_preserves_status_color_on_bullet() {
    let foreground = AnsiColorEnum::Rgb(RgbColor(0xCC, 0xCC, 0xCC));
    let mut session = Session::new(
        InlineTheme {
            foreground: Some(foreground),
            tool_body: Some(AnsiColorEnum::Ansi(anstyle::AnsiColor::Green)),
            ..Default::default()
        },
        None,
        VIEW_ROWS,
    );
    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "• Ran find src/agent -type f".to_string(),
            style: Arc::new(InlineTextStyle {
                color: Some(AnsiColorEnum::Ansi(anstyle::AnsiColor::Red)),
                ..InlineTextStyle::default()
            }),
        }],
    );

    let rendered = session.reflow_pty_lines(0, 80);
    let bullet_span = rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.as_ref() == "• ")
        .expect("expected • span");
    let command_span = rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("find"))
        .expect("expected command span");

    assert_eq!(bullet_span.style.fg, Some(Color::Red));
    assert_eq!(command_span.style.fg, Some(Color::Rgb(0xCC, 0xCC, 0xCC)));
}
#[test]
fn pty_multiline_command_header_body_matches_single_line_tool_header() {
    // Screenshot 2026-09-18: a wrapped `• Ran ...` header with `│`
    // continuations rendered shell token colors (bold `cargo`/`echo`) while
    // single-line Tool headers stay uniform. Both header lines must render
    // as uniform non-bold foreground text: status bullet, bold verb only.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let bold_command = Arc::new(InlineTextStyle {
        color: Some(AnsiColorEnum::Ansi(anstyle::AnsiColor::Green)),
        effects: anstyle::Effects::BOLD,
        ..InlineTextStyle::default()
    });
    let option = Arc::new(InlineTextStyle {
        color: Some(AnsiColorEnum::Ansi(anstyle::AnsiColor::Red)),
        ..InlineTextStyle::default()
    });
    let plain = Arc::new(InlineTextStyle::default());
    // Shell-highlighted multi-segment header, as produced by live PTY
    // segment splitting: bullet, verb, body with bold command + option.
    session.push_line(
        InlineMessageKind::Pty,
        vec![
            InlineSegment {
                text: "• ".to_string(), style: Arc::clone(&plain)
            },
            InlineSegment { text: "Ran".to_string(), style: Arc::clone(&plain) },
            InlineSegment { text: " ".to_string(), style: Arc::clone(&plain) },
            InlineSegment {
                text: "cargo".to_string(),
                style: Arc::clone(&bold_command),
            },
            InlineSegment {
                text: " nextest run".to_string(),
                style: Arc::clone(&plain),
            },
            InlineSegment {
                text: " -p".to_string(),
                style: Arc::clone(&option),
            },
        ],
    );
    // Wrapped `│` continuation carrying shell token styles, including bold.
    session.push_line(
        InlineMessageKind::Pty,
        vec![
            InlineSegment { text: "  ".to_string(), style: Arc::clone(&plain) },
            InlineSegment { text: "│".to_string(), style: Arc::clone(&plain) },
            InlineSegment { text: " ".to_string(), style: Arc::clone(&plain) },
            InlineSegment {
                text: "/tmp/theme6.log".to_string(),
                style: Arc::clone(&plain),
            },
            InlineSegment { text: " ".to_string(), style: Arc::clone(&plain) },
            InlineSegment {
                text: "echo".to_string(),
                style: Arc::clone(&bold_command),
            },
        ],
    );

    let theme_fg = InlineTheme::default().foreground.map(ratatui_color_from_ansi);
    let first = session.reflow_pty_lines(0, 80);
    let first_spans: Vec<_> = first.iter().flat_map(|line| line.line.spans.iter()).collect();
    let first_text: String = first_spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(first_text.contains("• Ran cargo nextest run -p"), "header text preserved, got: {first_text:?}");
    for span in &first_spans {
        let content: &str = span.content.as_ref();
        if content.is_empty() {
            continue;
        }
        if content == "Ran" {
            assert!(span.style.add_modifier.contains(Modifier::BOLD), "verb stays bold");
        } else if content == "• " {
            assert!(
                !span.style.add_modifier.contains(Modifier::BOLD),
                "bullet not bold, got {:?}",
                span.style.add_modifier
            );
        } else {
            assert!(
                !span.style.add_modifier.contains(Modifier::BOLD),
                "header body not bold, got {:?} for {content:?}",
                span.style.add_modifier
            );
            assert_eq!(span.style.fg, theme_fg, "header body uses foreground, got {:?} for {content:?}", span.style.fg);
        }
    }

    let second = session.reflow_pty_lines(1, 80);
    let second_spans: Vec<_> = second.iter().flat_map(|line| line.line.spans.iter()).collect();
    let second_text: String = second_spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(second_text.contains("/tmp/theme6.log"), "continuation path kept, got: {second_text:?}");
    assert!(second_text.contains("echo"), "continuation command kept, got: {second_text:?}");
    for span in &second_spans {
        let content: &str = span.content.as_ref();
        if content.trim().is_empty() {
            continue;
        }
        assert!(
            !span.style.add_modifier.contains(Modifier::BOLD),
            "continuation not bold, got {:?} for {content:?}",
            span.style.add_modifier
        );
        assert_eq!(span.style.fg, theme_fg, "continuation uses foreground, got {:?} for {content:?}", span.style.fg);
    }
}
#[test]
fn pty_pipe_prefixed_output_after_output_keeps_pty_style() {
    // A `│`-prefixed row that follows output (not a header) must keep PTY
    // body styling instead of chaining onto the uniform header treatment.
    let foreground = AnsiColorEnum::Rgb(RgbColor(0xCC, 0xCC, 0xCC));
    let pty_body = AnsiColorEnum::Rgb(RgbColor(0x66, 0x66, 0x66));
    let mut session = Session::new(
        InlineTheme {
            foreground: Some(foreground),
            pty_body: Some(pty_body),
            ..Default::default()
        },
        None,
        VIEW_ROWS,
    );
    session.push_line(InlineMessageKind::Pty, vec![make_pty_segment("  └ some output")]);
    session.push_line(InlineMessageKind::Pty, vec![make_pty_segment("  │ tree output")]);

    let rendered = session.reflow_pty_lines(1, 80);
    let tree_span = rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("tree"))
        .expect("expected tree span");
    assert_eq!(tree_span.style.fg, Some(Color::Rgb(0x66, 0x66, 0x66)));
}
#[test]
fn pty_pipe_prefixed_output_after_wrapped_header_keeps_pty_style() {
    let foreground = AnsiColorEnum::Rgb(RgbColor(0xCC, 0xCC, 0xCC));
    let pty_body = AnsiColorEnum::Rgb(RgbColor(0x66, 0x66, 0x66));
    let mut session = Session::new(
        InlineTheme {
            foreground: Some(foreground),
            pty_body: Some(pty_body),
            ..Default::default()
        },
        None,
        VIEW_ROWS,
    );
    session.push_line(InlineMessageKind::Pty, vec![make_pty_segment("• Ran long command")]);
    session.push_line(InlineMessageKind::Pty, vec![make_pty_segment("  │ wrapped argument")]);
    // The four-space output gutter is distinct from the two-space header
    // continuation prefix, even when the program output starts with `│`.
    session.push_line(InlineMessageKind::Pty, vec![make_pty_segment("    │ program output")]);

    let rendered = session.reflow_pty_lines(2, 80);
    let output_span = rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("program output"))
        .expect("expected pipe-prefixed output span");
    assert_eq!(output_span.style.fg, Some(Color::Rgb(0x66, 0x66, 0x66)));
}
#[test]
fn tool_command_header_does_not_use_accent_tool_body_as_fallback() {
    let foreground = AnsiColorEnum::Rgb(RgbColor(0xCC, 0xCC, 0xCC));
    let mut session = Session::new(
        InlineTheme {
            foreground: Some(foreground),
            tool_body: Some(AnsiColorEnum::Ansi(anstyle::AnsiColor::Green)),
            ..Default::default()
        },
        None,
        VIEW_ROWS,
    );
    session.push_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text: "• Ran find src/agent -type f".to_string(),
            style: Arc::new(InlineTextStyle {
                color: Some(AnsiColorEnum::Ansi(anstyle::AnsiColor::Green)),
                ..InlineTextStyle::default()
            }),
        }],
    );

    let rendered = session.reflow_transcript_lines(80);
    let verb_span = rendered
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.as_ref() == "Ran")
        .expect("expected Ran span");
    let command_span = rendered
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.contains("find"))
        .expect("expected command span");

    assert_eq!(verb_span.style.fg, Some(Color::Rgb(0xCC, 0xCC, 0xCC)));
    assert_eq!(command_span.style.fg, Some(Color::Rgb(0xCC, 0xCC, 0xCC)));
    assert!(!verb_span.style.add_modifier.contains(Modifier::DIM));
    assert!(!command_span.style.add_modifier.contains(Modifier::DIM));
}
#[test]
fn tool_actions_use_theme_semantic_colors_instead_of_terminal_palette_colors() {
    let primary = AnsiColorEnum::Rgb(RgbColor(0x70, 0x90, 0xB0));
    let tool_accent = AnsiColorEnum::Rgb(RgbColor(0xD0, 0x70, 0x60));
    let mut session = Session::new(
        InlineTheme {
            foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xF0, 0xF0, 0xF0))),
            primary: Some(primary),
            tool_accent: Some(tool_accent),
            ..InlineTheme::default()
        },
        None,
        VIEW_ROWS,
    );
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Write file.rs")]);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• git status")]);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• version_control status")]);

    let rendered = session.reflow_transcript_lines(80);
    for (action_name, expected_color) in [("Write", tool_accent), ("git", primary), ("version_control", primary)] {
        let action = rendered
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content.as_ref() == action_name)
            .unwrap_or_else(|| panic!("tool action span {action_name:?}"));

        assert_eq!(action.style.fg, Some(ratatui_color_from_ansi(expected_color)));
        assert!(action.style.add_modifier.contains(Modifier::BOLD));
    }
}
#[test]
fn tool_output_is_not_dimmed_and_tool_header_is_opaque() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text: "• Ran sed -n 1,260p\n    output line".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let rendered = session.reflow_transcript_lines(80);
    let header = rendered
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.as_ref() == "Ran")
        .expect("expected tool header");
    let output = rendered
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.contains("output line"))
        .expect("expected tool output");

    assert!(!header.style.add_modifier.contains(Modifier::DIM));
    assert!(!output.style.add_modifier.contains(Modifier::DIM));
}
#[test]
fn exit_status_row_uses_success_and_error_accents() {
    let theme = InlineTheme {
        foreground: Some(anstyle::AnsiColor::White.into()),
        background: Some(anstyle::AnsiColor::Black.into()),
        primary: Some(anstyle::AnsiColor::Green.into()),
        error: Some(anstyle::AnsiColor::Red.into()),
        ..InlineTheme::default()
    };

    let badge_foreground = |text: &str| {
        let mut session = Session::new(theme.clone(), None, VIEW_ROWS);
        session.push_line(
            InlineMessageKind::Tool,
            vec![InlineSegment {
                text: text.to_string(),
                style: Arc::new(InlineTextStyle::default()),
            }],
        );
        let rendered = session.reflow_transcript_lines(80);
        let badge = rendered
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content.contains(text))
            .unwrap_or_else(|| panic!("expected exit badge span for {text}"));
        assert!(!badge.style.add_modifier.contains(Modifier::DIM), "exit badge must not be dimmed: {text}");
        badge.style.fg
    };

    // Clean exit resolves to the success accent; any non-zero code to the error
    // accent. Both stay at normal intensity and keep the `✓ exit N` text.
    assert_eq!(badge_foreground("✓ exit 0"), Some(ratatui_color_from_ansi(anstyle::AnsiColor::Green.into())));
    assert_eq!(badge_foreground("✓ exit 1"), Some(ratatui_color_from_ansi(anstyle::AnsiColor::Red.into())));
}
#[test]
fn pty_lines_use_subdued_foreground() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        background: Some(AnsiColorEnum::Ansi(anstyle::AnsiColor::Black)),
        pty_body: Some(AnsiColorEnum::Rgb(RgbColor(0x7A, 0x7A, 0x7A))),
        ..InlineTheme::default()
    };
    let mut session = Session::new(theme, None, VIEW_ROWS);
    push_pty_line(&mut session, "plain pty output");

    let rendered = session.reflow_pty_lines(0, 80);
    let body_span = rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("plain pty output"))
        .expect("expected PTY body span");
    assert_eq!(
        body_span.style.fg,
        Some(Color::Rgb(0x7A, 0x7A, 0x7A)),
        "PTY body span should use the subdued pty_body foreground"
    );
    assert!(!body_span.style.add_modifier.contains(Modifier::DIM), "PTY body should render at normal intensity");
}
#[test]
fn assistant_text_is_brighter_than_pty_output() {
    let agent_fg = Color::Rgb(0xEE, 0xEE, 0xEE);
    let pty_fg = Color::Rgb(0x7A, 0x7A, 0x7A);
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        pty_body: Some(AnsiColorEnum::Rgb(RgbColor(0x7A, 0x7A, 0x7A))),
        ..Default::default()
    };

    let mut session = Session::new(theme, None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "assistant reply".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "pty output".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let agent_spans = session.render_message_spans(0);
    let agent_body = agent_spans
        .iter()
        .find(|span| span.content.contains("assistant reply"))
        .expect("expected assistant body span");
    assert_eq!(agent_body.style.fg, Some(agent_fg));

    let pty_rendered = session.reflow_pty_lines(1, 80);
    let pty_body = pty_rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("pty output"))
        .expect("expected PTY body span");
    assert_eq!(pty_body.style.fg, Some(pty_fg));
    assert!(!pty_body.style.add_modifier.contains(Modifier::DIM));
    assert_ne!(agent_body.style.fg, pty_body.style.fg);
}
#[test]
fn pty_ansi_detail_colors_keep_full_intensity() {
    let mut session = Session::new(
        InlineTheme {
            background: Some(AnsiColorEnum::Ansi(anstyle::AnsiColor::Black)),
            pty_body: Some(AnsiColorEnum::Rgb(RgbColor(0x7A, 0x7A, 0x7A))),
            ..Default::default()
        },
        None,
        VIEW_ROWS,
    );
    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "SUCCESS: Code formatting is correct!".to_string(),
            style: Arc::new(InlineTextStyle {
                color: Some(AnsiColorEnum::Ansi(anstyle::AnsiColor::BrightGreen)),
                ..InlineTextStyle::default()
            }),
        }],
    );

    let rendered = session.reflow_pty_lines(0, 80);
    let body_span = rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("SUCCESS"))
        .expect("expected ANSI-colored PTY detail span");

    assert!(body_span.style.fg.is_some());
    assert_ne!(
        body_span.style.fg,
        Some(Color::Rgb(55, 165, 55)),
        "explicit PTY detail colors must no longer be attenuated toward the background"
    );
    assert!(!body_span.style.add_modifier.contains(Modifier::DIM));
}
#[test]
fn pty_scroll_preserves_order() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    for index in 0..200 {
        let label = format!("{LABEL_PREFIX}-{index}");
        push_pty_line(&mut session, &label);
    }

    let bottom_view = visible_transcript(&mut session);
    assert!(
        bottom_view.iter().any(|line| line.contains(&format!("{LABEL_PREFIX}-199"))),
        "bottom view should include latest PTY line"
    );

    for _ in 0..200 {
        session.scroll_page_up();
        if session.scroll_manager.offset() == session.current_max_scroll_offset() {
            break;
        }
    }

    let top_view = visible_transcript(&mut session);
    assert!(
        (0..=5).any(|index| top_view.iter().any(|line| line.contains(&format!("{LABEL_PREFIX}-{index}")))),
        "top view should include earliest PTY lines"
    );
    assert!(
        top_view.iter().all(|line| !line.contains(&format!("{LABEL_PREFIX}-199"))),
        "top view should not include latest PTY line"
    );
}
