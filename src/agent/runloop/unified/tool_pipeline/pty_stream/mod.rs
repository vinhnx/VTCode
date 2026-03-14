mod runtime;
mod segments;
mod state;

pub(crate) use runtime::PtyStreamRuntime;

#[cfg(test)]
mod tests {
    use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects};
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    use tokio::sync::oneshot;
    use tokio::time::timeout;
    use vtcode_tui::InlineSegment;

    use super::runtime::PtyStreamRuntime;
    use super::segments::{PtyLineStyles, line_to_segments, tokenize_preserve_whitespace};
    use super::state::PtyStreamState;

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

    #[test]
    fn pty_stream_state_streams_incremental_chunks() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("line1\nline2", 5);
        let rendered = state.render_lines(5);
        assert_eq!(
            rendered,
            vec!["  └ line1".to_string(), "    line2".to_string()]
        );
        assert_eq!(state.last_display_line(), Some("line2".to_string()));
    }

    #[test]
    fn pty_stream_state_handles_carriage_return_overwrite() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("start\rreplace\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["  └ replace".to_string()]);
        assert_eq!(state.last_display_line(), Some("replace".to_string()));
    }

    #[test]
    fn pty_stream_state_applies_tail_truncation() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("a\nb\nc\nd\ne\nf\ng\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(
            rendered,
            vec![
                "  └ a".to_string(),
                "    b".to_string(),
                "    c".to_string(),
                "    … +1 line".to_string(),
                "    e".to_string(),
                "    f".to_string(),
                "    g".to_string(),
            ]
        );
    }

    #[test]
    fn pty_stream_state_formats_hidden_line_summary() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("a\nb\nc\nd\ne\nf\ng\nh\n", 5);
        let rendered = state.render_lines(5);
        assert!(rendered.contains(&"    … +2 lines".to_string()));
    }

    #[test]
    fn pty_stream_state_deduplicates_consecutive_lines() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("same\nsame\nnext\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(
            rendered,
            vec!["  └ same".to_string(), "    next".to_string()]
        );
    }

    #[test]
    fn pty_stream_state_preserves_indentation_and_blank_lines() {
        let mut state = PtyStreamState::new(None);
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
    fn pty_stream_state_renders_command_prompt_without_output() {
        let state = PtyStreamState::new(Some("cargo check".to_string()));
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["• Ran cargo check".to_string()]);
    }

    #[test]
    fn pty_stream_state_keeps_command_prompt_with_truncated_tail() {
        let mut state = PtyStreamState::new(Some("cargo check".to_string()));
        state.apply_chunk("a\nb\nc\nd\ne\nf\ng\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(
            rendered,
            vec![
                "• Ran cargo check".to_string(),
                "  └ a".to_string(),
                "    b".to_string(),
                "    c".to_string(),
                "    … +1 line".to_string(),
                "    e".to_string(),
                "    f".to_string(),
                "    g".to_string(),
            ]
        );
    }

    #[test]
    fn normalizes_command_prompt_whitespace() {
        let state = PtyStreamState::new(Some("  cargo   check \n -p  vtcode  ".to_string()));
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["• Ran cargo check -p vtcode".to_string()]);
    }

    #[test]
    fn wraps_long_command_header() {
        let command = "cargo test -p vtcode run_command_preview_ build_tool_summary_formats_run_command_as_ran";
        let state = PtyStreamState::new(Some(command.to_string()));
        let rendered = state.render_lines(5);
        assert_eq!(rendered.len(), 2);
        assert!(rendered[0].starts_with("• Ran cargo test -p vtcode run_command_preview_"));
        assert!(rendered[1].starts_with("  │ build_tool_summary_formats_run_command_as_ran"));
    }

    #[test]
    fn tokenization_preserves_whitespace() {
        let tokens = tokenize_preserve_whitespace("cargo   check -p  vtcode");
        assert_eq!(
            tokens,
            vec!["cargo", "   ", "check", " ", "-p", "  ", "vtcode"]
        );
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
        assert_eq!(
            err_segment.style.color,
            Some(AnsiColorEnum::Ansi(AnsiColor::Red))
        );
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
    fn line_to_segments_stdout_defaults_to_dimmed_style() {
        let styles = PtyLineStyles::new();
        let (segments, _) = line_to_segments("  └ cargo check done", &styles);
        let output_segment = segments
            .iter()
            .find(|segment| segment.text.contains("cargo check done"))
            .expect("stdout segment should be present");
        assert!(output_segment.style.effects.contains(Effects::DIMMED));
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
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("\u{1b}]8;;https://example.com/docs\u{1b}\\", 5);

        let (_, segments, link_ranges, _) = state.render_segments("docs\u{1b}]8;;\u{1b}\\\n", 5);
        assert_eq!(segments.len(), 1);
        assert_eq!(flatten_text(&segments[0]), "  └ docs");
        assert_eq!(link_ranges.len(), 1);
        assert_eq!(link_ranges[0].len(), 1);
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
        let runtime = PtyStreamRuntime {
            sender: None,
            task: Some(task),
            active: Arc::clone(&active),
        };

        drop(runtime);

        assert!(!active.load(Ordering::Relaxed));
        timeout(Duration::from_millis(300), drop_rx)
            .await
            .expect("background task should be aborted on drop")
            .expect("drop notifier should signal when task future is dropped");
    }
}
