use super::*;

#[tokio::test]
async fn exec_session_body_is_bounded_and_points_at_the_transcript() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let content = (1..=40).map(|n| format!("build log line {n}")).collect::<Vec<_>>().join("\n");

    let git_styles = GitStyles::new();
    let ls_styles = LsStyles::from_env();

    render_stream_section(
        &mut renderer,
        "",
        &content,
        ToolOutputMode::Compact,
        30,
        Some(tool_names::WRITE_STDIN),
        &git_styles,
        &ls_styles,
        MessageStyle::ToolOutput,
        false,
        true,
        None,
    )
    .await
    .expect("session body should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    let content_lines = output.lines().filter(|line| line.contains("build log line")).count();
    assert_eq!(
        content_lines, EXEC_SESSION_OUTPUT_MAX_LINES,
        "session body must be capped at {EXEC_SESSION_OUTPUT_MAX_LINES} rows: {output:?}"
    );
    // Tail-biased: the exit/summary rows survive the trim.
    assert!(output.contains("build log line 40"), "tail should survive: {output:?}");
    assert!(!output.contains("build log line 1\n"), "head should be trimmed: {output:?}");
    // The hidden-row count and the expand affordance stay discoverable.
    assert!(output.contains("+30 lines"), "hidden count should be shown: {output:?}");
    assert!(output.contains("click to expand"), "expand affordance should be shown: {output:?}");
}
#[tokio::test]
async fn exec_session_body_keeps_short_output_whole() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());

    render_stream_section(
        &mut renderer,
        "",
        "line 1\nline 2\nline 3",
        ToolOutputMode::Compact,
        30,
        Some(tool_names::WRITE_STDIN),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolOutput,
        false,
        true,
        None,
    )
    .await
    .expect("short session body should render");

    let collected = collect_inline_output(&mut receiver);
    let output = strip_ansi_codes(&collected);
    assert!(output.contains("line 1") && output.contains("line 2") && output.contains("line 3"));
    assert!(!output.contains("truncated") && !output.contains("lines ("), "no notice expected: {output:?}");
}
#[tokio::test]
async fn exec_session_body_and_notice_are_dimmed() {
    use anstyle::Effects;
    use vtcode_core::ui::InlineCommand;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let content = (1..=40).map(|n| format!("build log line {n}")).collect::<Vec<_>>().join("\n");

    render_stream_section(
        &mut renderer,
        "",
        &content,
        ToolOutputMode::Compact,
        30,
        Some(tool_names::WRITE_STDIN),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolOutput,
        false,
        true,
        None,
    )
    .await
    .expect("session body should render");

    let mut body_segment = None;
    let mut notice_segment = None;
    while let Ok(command) = receiver.try_recv() {
        let segments = match command {
            InlineCommand::AppendLine { segments, .. } | InlineCommand::AppendToolOutputLine { segments, .. } => {
                segments
            }
            _ => continue,
        };
        let text = segments.iter().map(|s| s.text.as_str()).collect::<String>();
        if body_segment.is_none() && text.contains("build log line") {
            body_segment = segments.first().cloned();
        }
        if notice_segment.is_none() && text.contains("click to expand") {
            // The underlined action phrase is its own segment.
            notice_segment = segments.into_iter().find(|s| s.text.contains("click to expand"));
        }
    }

    let body = body_segment.expect("session body segment");
    assert!(body.style.effects.contains(Effects::DIMMED), "session body must be dimmed: {:?}", body.style);
    let notice = notice_segment.expect("expand notice segment");
    assert!(
        notice.style.effects.contains(Effects::UNDERLINE),
        "expand action must be underlined as the click target: {:?}",
        notice.style
    );
}
#[tokio::test]
async fn exec_session_expand_notice_binds_tool_output_id() {
    use vtcode_core::ui::InlineCommand;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_session_expand_anchor(42);
    let content = (1..=40).map(|n| format!("build log line {n}")).collect::<Vec<_>>().join("\n");

    render_stream_section(
        &mut renderer,
        "",
        &content,
        ToolOutputMode::Compact,
        30,
        Some(tool_names::WRITE_STDIN),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolOutput,
        false,
        true,
        None,
    )
    .await
    .expect("session body should render");

    let mut bound_notice_id = None;
    while let Ok(command) = receiver.try_recv() {
        if let InlineCommand::AppendToolOutputLine { id, segments, .. } = command {
            let text = segments.iter().map(|s| s.text.as_str()).collect::<String>();
            if text.contains("click to expand") {
                bound_notice_id = Some(id);
            }
        }
    }
    assert_eq!(
        bound_notice_id,
        Some(42),
        "expand notice must carry the recorded capture id so click opens the viewer"
    );
}
#[tokio::test]
async fn exec_session_body_skips_ls_and_diff_coloring() {
    // Build-test lines mentioning `.rs` must not pick up LS_COLORS file-type
    // colors, and a diff-shaped session capture stays plain: the body is
    // terminal text, not an `ls` listing or a tool diff.
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let content = "PASS  [0.013s] vtcode-core/src/tools/exec_session.rs\n+let added = 1;\n-let removed = 2;\n";

    render_stream_section(
        &mut renderer,
        "",
        content,
        ToolOutputMode::Compact,
        30,
        Some(tool_names::WRITE_STDIN),
        &GitStyles::new(),
        &LsStyles::from_env(),
        MessageStyle::ToolOutput,
        false,
        true,
        None,
    )
    .await
    .expect("session body should render");

    // Styles live on the emitted segments; `collect_inline_output` drops them,
    // so read the inline commands directly. Both the line text and its indent
    // prefix carry the applied style, so per-line coloring changes either.
    let session_styles = collect_inline_line_styles(&mut receiver).await;
    let unique: std::collections::HashSet<_> = session_styles.iter().cloned().collect();
    assert_eq!(
        unique.len(),
        1,
        "session body must use one plain style, not per-line git/ls coloring: {session_styles:?}"
    );

    // Control: the same content rendered as a real diff body does vary per
    // line, so the assertion above cannot pass vacuously.
    let control_styles = collect_diff_body_styles(content).await;
    assert!(
        control_styles.iter().collect::<std::collections::HashSet<_>>().len() > 1,
        "control must show per-line coloring for the assertion to mean anything: {control_styles:?}"
    );
}
