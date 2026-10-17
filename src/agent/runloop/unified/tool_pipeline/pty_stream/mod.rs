mod runtime;
mod segments;
mod state;

pub(crate) use runtime::PtyStreamRuntime;

#[cfg(test)]
mod tests {
    use anstyle::{AnsiColor, Color as AnsiColorEnum};
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use tokio::sync::{mpsc, oneshot};
    use tokio::time::timeout;
    use vtcode_core::config::PtyConfig;
    use vtcode_ui::tui::app::{InlineCommand, InlineHandle, InlineSegment};

    use super::runtime::PtyStreamRuntime;
    use super::segments::{PtyLineStyles, line_to_segments};
    use super::state::PtyStreamState;
    use crate::agent::runloop::unified::progress::ProgressReporter;
    use vtcode_ui::tui::ui::shell_syntax::tokenize_preserve_whitespace;

    struct DropNotifier(Option<oneshot::Sender<()>>);

    impl Drop for DropNotifier {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }

    fn flatten_text(segments: &[InlineSegment]) -> String {
        segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join("")
    }

    fn test_pty_config() -> PtyConfig {
        PtyConfig::default()
    }

    #[test]
    fn pty_stream_state_streams_incremental_chunks() {
        let mut state = PtyStreamState::new(None, test_pty_config(), None);
        state.apply_chunk("line1\nline2", 5);
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["  └ line1".to_string(), "    line2".to_string()]);
        assert_eq!(state.last_display_line(5), Some("line2".to_string()));
    }

    #[test]
    fn pty_stream_state_handles_carriage_return_overwrite() {
        let mut state = PtyStreamState::new(None, test_pty_config(), None);
        state.apply_chunk("start\rreplace\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["  └ replace".to_string()]);
        assert_eq!(state.last_display_line(5), Some("replace".to_string()));
    }

    #[test]
    fn pty_stream_state_applies_tail_truncation() {
        let mut state = PtyStreamState::new(None, test_pty_config(), None);
        state.apply_chunk("a\nb\nc\nd\ne\nf\ng\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(
            rendered,
            vec![
                "  └ a".to_string(),
                "    b".to_string(),
                "    … +3 lines".to_string(),
                "    f".to_string(),
                "    g".to_string(),
            ]
        );
    }

    #[test]
    fn pty_stream_state_formats_hidden_line_summary() {
        let mut state = PtyStreamState::new(None, test_pty_config(), None);
        state.apply_chunk("a\nb\nc\nd\ne\nf\ng\nh\n", 5);
        let rendered = state.render_lines(5);
        assert!(rendered.contains(&"    … +4 lines".to_string()));
    }

    #[test]
    fn pty_stream_state_preserves_consecutive_duplicate_lines() {
        let mut state = PtyStreamState::new(None, test_pty_config(), None);
        state.apply_chunk("same\nsame\nnext\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["  └ same".to_string(), "    same".to_string(), "    next".to_string(),]);
    }

    #[test]
    fn pty_stream_state_preserves_indentation_and_blank_lines() {
        let mut state = PtyStreamState::new(None, test_pty_config(), None);
        state.apply_chunk("  fn main() {\n\n    println!(\"hi\");\n  }\n", 8);
        let rendered = state.render_lines(8);
        assert_eq!(
            rendered,
            vec![
                "  └   fn main() {".to_string(),
                "    ".to_string(),
                "        println!(\"hi\");".to_string(),
                "      }".to_string(),
            ]
        );
    }

    #[test]
    fn pty_stream_state_bounds_newline_free_line_and_recovers_on_next_line() {
        let mut state = PtyStreamState::new(None, test_pty_config(), None);
        for _ in 0..8 {
            state.apply_chunk(&"x".repeat(4096), 5);
        }
        let last = state.last_display_line(5).expect("line expected");
        assert!(last.len() <= 8 * 1024 + '…'.len_utf8());
        assert!(last.ends_with('…'));

        state.apply_chunk("\nok\n", 5);
        assert_eq!(state.last_display_line(5), Some("ok".to_string()));
    }

    #[test]
    fn pty_stream_state_line_cap_respects_utf8_boundaries() {
        let mut state = PtyStreamState::new(None, test_pty_config(), None);
        state.apply_chunk(&"é".repeat(8 * 1024), 5);
        let last = state.last_display_line(5).expect("line expected");
        assert!(last.ends_with('…'));
        assert!(last.chars().filter(|c| *c == 'é').count() * 2 <= 8 * 1024);
    }

    #[test]
    fn pty_stream_state_renders_command_prompt_without_output() {
        let state = PtyStreamState::new(Some("cargo check".to_string()), test_pty_config(), None);
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["• Ran cargo check".to_string()]);
    }

    #[test]
    fn pty_stream_state_uses_bounded_live_preview() {
        let mut state = PtyStreamState::new(Some("cargo check".to_string()), test_pty_config(), None);
        state.apply_chunk("first\nsecond\nthird\n", 2);

        assert_eq!(
            state.render_lines(2),
            vec![
                "• Ran cargo check".to_string(),
                "    … +1 line".to_string(),
                "  └ second".to_string(),
                "    third".to_string(),
            ]
        );
    }

    #[test]
    fn pty_stream_state_keeps_command_prompt_with_truncated_tail() {
        let mut state = PtyStreamState::new(Some("cargo check".to_string()), test_pty_config(), None);
        state.apply_chunk("a\nb\nc\nd\ne\nf\ng\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(
            rendered,
            vec![
                "• Ran cargo check".to_string(),
                "  └ a".to_string(),
                "    b".to_string(),
                "    … +3 lines".to_string(),
                "    f".to_string(),
                "    g".to_string(),
            ]
        );
    }

    #[test]
    fn normalizes_command_prompt_whitespace() {
        let state = PtyStreamState::new(Some("  cargo   check \n -p  vtcode  ".to_string()), test_pty_config(), None);
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["• Ran cargo check -p vtcode".to_string()]);
    }

    #[test]
    fn wraps_long_command_header() {
        let command = "cargo test -p vtcode run_command_preview_ build_tool_summary_formats_run_command_as_ran";
        let state = PtyStreamState::new(Some(command.to_string()), test_pty_config(), None);
        let rendered = state.render_lines(5);
        assert_eq!(rendered.len(), 2);
        assert!(rendered[0].starts_with("• Ran cargo test -p vtcode run_command_preview_"));
        assert!(rendered[1].starts_with("  │ build_tool_summary_formats_run_command_as_ran"));
    }

    #[test]
    fn screenshot_grep_pipeline_header_renders_in_full_without_truncation() {
        // Screenshot 2026-09-24 16:37: `• Ran grep -rn "@vinhnx/..." docs`
        // wrapped across `│` lines must keep every pipe segment with no `…`.
        // Exact screenshot bytes: `||` inside the quoted pattern and the
        // backslash-escaped `\.backup` arg must both survive (3 pattern pipes
        // + 3 shell pipes = 6).
        let command = "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs | grep -v node_modules | grep -v package-lock | grep -v \"\\.backup\"";
        assert!(command.chars().count() > 120, "fixture must overflow the old preview cap");
        let state = PtyStreamState::new(Some(command.to_string()), test_pty_config(), None);
        let rendered = state.render_lines(8);
        let joined = rendered.join("\n");
        assert!(!joined.contains('…'), "command header must not truncate, got: {joined:?}");
        assert!(joined.contains("node_modules"), "got: {joined:?}");
        assert!(joined.contains("package-lock"), "got: {joined:?}");
        assert!(joined.contains("\"\\.backup\""), "final pipe arg must survive, got: {joined:?}");
        assert_eq!(joined.matches('|').count(), 6, "pattern pipes + shell pipes must survive: {joined:?}");
        // Proper shell-aware wrapping: the quoted pattern holds spaces but
        // must stay on the first line, never split mid-quote, and every
        // segment must fit its 62/58 budget plus the `• Ran` / `  │ ` prefix.
        assert!(
            rendered[0].contains("\"@vinhnx/vtcode|npm install -g||npx @vinhnx\""),
            "quoted pattern must stay atomic on the first line, got: {rendered:?}"
        );
        for (index, line) in rendered.iter().enumerate() {
            let (body, budget) = if index == 0 {
                (line.strip_prefix("• Ran ").expect("header prefix"), 62)
            } else {
                (line.strip_prefix("  │ ").expect("continuation prefix"), 58)
            };
            assert!(body.chars().count() <= budget, "line {index} exceeds its {budget}-char budget: {line:?}");
        }
    }

    #[test]
    fn pty_stream_state_uses_terminal_snapshot_for_screen_rewrites() {
        let mut state = PtyStreamState::new(None, test_pty_config(), None);
        state.apply_chunk("before\n\x1b[2J\x1b[Hmenu\nitem\n", 6);

        assert_eq!(state.render_lines(6), vec!["  └ menu".to_string(), "    item".to_string()]);
        assert_eq!(state.last_display_line(6), Some("item".to_string()));
    }

    #[test]
    fn tokenization_preserves_whitespace() {
        let tokens = tokenize_preserve_whitespace("cargo   check -p  vtcode");
        assert_eq!(tokens, vec!["cargo", "   ", "check", " ", "-p", "  ", "vtcode"]);
    }

    #[test]
    fn line_to_segments_preserves_command_text() {
        let styles = PtyLineStyles::new();
        let line = "• Ran echo \"$HOME\" && cargo check";
        let (segments, _) = line_to_segments(line, &styles);
        assert_eq!(flatten_text(&segments), line);
    }

    #[test]
    fn line_to_segments_distinguishes_command_and_args_styles() {
        let styles = PtyLineStyles::new();
        let (segments, _) = line_to_segments("• Ran cargo fmt", &styles);
        assert_eq!(flatten_text(&segments), "• Ran cargo fmt");
        assert!(
            segments
                .iter()
                .any(|segment| !segment.text.trim().is_empty() && segment.style.color.is_some())
        );
    }

    #[test]
    fn line_to_segments_handles_invalid_bash_input_without_dropping_text() {
        let styles = PtyLineStyles::new();
        let (segments, _) = line_to_segments("• Ran )(", &styles);
        assert_eq!(flatten_text(&segments), "• Ran )(");
    }

    #[test]
    fn line_to_segments_preserves_stdout_ansi_styles() {
        let styles = PtyLineStyles::new();
        let (segments, _) = line_to_segments("  └ \u{1b}[31mERR\u{1b}[0m done", &styles);
        assert_eq!(flatten_text(&segments), "  └ ERR done");

        let err_segment = segments
            .iter()
            .find(|segment| segment.text.contains("ERR"))
            .expect("colored text segment should be present");
        assert_eq!(err_segment.style.color, Some(AnsiColorEnum::Ansi(AnsiColor::Red)));
    }

    #[test]
    fn line_to_segments_ignores_non_sgr_ansi_sequences_without_dropping_text() {
        let styles = PtyLineStyles::new();
        let (segments, _) = line_to_segments("  └ \u{1b}[2Kclean", &styles);
        assert_eq!(flatten_text(&segments), "  └ clean");
        let clean_segment = segments
            .iter()
            .find(|segment| segment.text.contains("clean"))
            .expect("text segment should be present");
        assert_eq!(*clean_segment.style, *styles.output);
    }

    #[test]
    fn line_to_segments_stdout_uses_output_style() {
        let styles = PtyLineStyles::new();
        let (segments, _) = line_to_segments("  └ cargo check done", &styles);
        let output_segment = segments
            .iter()
            .find(|segment| segment.text.contains("cargo check done"))
            .expect("stdout segment should be present");
        assert_eq!(*output_segment.style, *styles.output);
    }

    #[test]
    fn line_to_segments_continuation_line_keeps_first_token_as_arg_style() {
        let styles = PtyLineStyles::new();
        let (segments, _) = line_to_segments("  │ --flag value", &styles);
        assert_eq!(flatten_text(&segments), "  │ --flag value");
        assert!(
            segments
                .iter()
                .any(|segment| !segment.text.trim().is_empty() && segment.style.color.is_some())
        );
    }

    #[test]
    fn pty_stream_state_preserves_osc8_links_across_control_only_chunks() {
        let mut state = PtyStreamState::new(None, test_pty_config(), None);
        state.apply_chunk("\u{1b}]8;;https://example.com/docs\u{1b}\\", 5);

        let (_, segments, link_ranges, _) = state.render_segments("docs\u{1b}]8;;\u{1b}\\\n", 5);
        assert_eq!(segments.len(), 1);
        assert_eq!(flatten_text(&segments[0]), "  └ docs");
        assert_eq!(link_ranges.len(), 1);
        assert_eq!(link_ranges[0].len(), 1);
    }

    #[tokio::test]
    async fn compact_pty_runtime_does_not_emit_live_preview() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let (runtime, callback) = PtyStreamRuntime::start(
            handle,
            Default::default(),
            8,
            Some("cargo check".to_string()),
            test_pty_config(),
            None,
            false,
        );

        callback("run_pty_cmd", "first\nsecond\n");
        runtime.shutdown(anstyle::Color::Ansi(AnsiColor::Green)).await;

        let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, InlineCommand::ReplaceLast { .. })),
            "compact PTY execution must not emit a transient live row"
        );
    }

    #[tokio::test]
    async fn compact_pty_runtime_streams_status_line_without_transcript() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let progress = ProgressReporter::new();
        let (runtime, callback) = PtyStreamRuntime::start(
            handle,
            progress.clone(),
            8,
            Some("cargo test".to_string()),
            test_pty_config(),
            None,
            false,
        );

        // Asymmetric pair: cargo-style progress line vs ANSI-only spinner frame.
        // Only the former must reach the status line; control sequences alone
        // must not overwrite it with blank text.
        callback("exec_command", "   Compiling vtcode-core v0.163.2\n");
        callback("exec_command", "\x1b[2K\x1b[1G");
        runtime.shutdown(anstyle::Color::Ansi(AnsiColor::Green)).await;

        let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, InlineCommand::ReplaceLast { .. })),
            "compact mode must keep the transcript stable while streaming status"
        );

        let info = progress.progress_info().await;
        assert!(
            info.message.contains("Compiling vtcode-core"),
            "compact mode must stream live stdout to status line, got: {:?}",
            info.message
        );
    }

    #[tokio::test]
    async fn enabled_pty_runtime_emits_live_preview() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let (runtime, callback) = PtyStreamRuntime::start(
            handle,
            Default::default(),
            8,
            Some("cargo check".to_string()),
            test_pty_config(),
            None,
            true,
        );

        callback("run_pty_cmd", "first\nsecond\n");
        runtime.shutdown(anstyle::Color::Ansi(AnsiColor::Green)).await;

        let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, InlineCommand::ReplaceLast { .. })),
            "expanded PTY execution should retain its live preview"
        );
    }

    #[tokio::test]
    async fn expanded_live_preview_never_exceeds_ten_rows() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let (runtime, callback) =
            PtyStreamRuntime::start(handle, Default::default(), 50, None, test_pty_config(), None, true);

        let chunk = (1..=15).map(|n| format!("line-{n:02}")).collect::<Vec<_>>().join("\n") + "\n";
        callback("run_pty_cmd", &chunk);
        runtime.shutdown(anstyle::Color::Ansi(AnsiColor::Green)).await;

        let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
        let last_preview = commands
            .iter()
            .rev()
            .find_map(|command| match command {
                InlineCommand::ReplaceLast { lines, .. } => Some(lines),
                _ => None,
            })
            .expect("expanded execution should emit a live preview");
        assert!(
            last_preview.len() <= 10,
            "live preview must stay within the 10-row budget, got {} rows",
            last_preview.len()
        );
        let text = last_preview
            .iter()
            .map(|row| row.iter().map(|segment| segment.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("line-01"), "head row should survive: {text:?}");
        assert!(text.contains("line-15"), "tail row should survive: {text:?}");
        assert!(!text.contains("line-05"), "middle row should be trimmed: {text:?}");
    }

    #[tokio::test]
    async fn pty_stream_runtime_drop_aborts_background_task() {
        let (drop_tx, drop_rx) = oneshot::channel();
        let notifier = DropNotifier(Some(drop_tx));
        let task = tokio::spawn(async move {
            let _notifier = notifier;
            std::future::pending::<()>().await;
        });
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let runtime = PtyStreamRuntime::for_test(task, Arc::clone(&active));

        drop(runtime);

        assert!(!active.load(Ordering::Relaxed));
        timeout(Duration::from_millis(300), drop_rx)
            .await
            .expect("background task should be aborted on drop")
            .expect("drop notifier should signal when task future is dropped");
    }
}
