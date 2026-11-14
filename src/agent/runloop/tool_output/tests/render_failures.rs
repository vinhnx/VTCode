//! Failed-result evidence policy: interactive transcripts keep only the
//! compact failed marker; the evidence block stays in the plain-text trail.
//! `Verbose` and headless surfaces keep the evidence inline.

use super::*;

#[tokio::test]
async fn failed_tool_keeps_compact_marker_and_trails_evidence() {
    transcript::clear();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "error": "file not found: src/missing.rs",
        "error_type": "ResourceNotFound",
        "path": "src/missing.rs"
    });

    render_tool_output(&mut renderer, Some("custom_tool"), &payload, None)
        .await
        .expect("failed result should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("✗ failed"), "compact marker paints");
    assert!(!inline_output.contains("Error: file not found"), "evidence block stays out of the transcript");
    assert!(!inline_output.contains("Type:"), "error type stays out of the transcript");

    let trail = transcript::snapshot().join("\n");
    assert!(trail.contains("Error: file not found: src/missing.rs"), "evidence joins the trail");
    assert!(trail.contains("Type: Resource not found"), "error type joins the trail");
    transcript::clear();
}

#[tokio::test]
async fn verbose_style_keeps_failure_evidence_inline() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_communication_style(CommunicationStyle::Verbose);

    render_tool_output(
        &mut renderer,
        Some("custom_tool"),
        &json!({
            "error": "file not found: src/missing.rs",
            "error_type": "ResourceNotFound"
        }),
        None,
    )
    .await
    .expect("failed result should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("Error: file not found: src/missing.rs"), "Verbose keeps the block");
    assert!(inline_output.contains("Type: Resource not found"), "Verbose keeps the type line");
}

#[test]
fn failure_evidence_gate_is_style_and_surface_dependent() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut inline = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    assert!(!inline.failure_evidence_visible(), "interactive Balanced hides evidence");
    inline.set_communication_style(CommunicationStyle::Verbose);
    assert!(inline.failure_evidence_visible(), "Verbose keeps evidence");

    let headless = AnsiRenderer::stdout();
    assert!(headless.failure_evidence_visible(), "headless output is its own surface");
}

#[tokio::test]
async fn failed_command_hides_stderr_and_reads_failed_badge() {
    transcript::clear();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "cargo build",
        "session_id": "run-1",
        "output": "compiling vtcode",
        "is_exited": true,
        "exit_code": 5,
        "stderr": "error: could not compile `vtcode`"
    });

    render_terminal_command_panel(&mut renderer, &payload, &GitStyles::new(), &LsStyles::from_env(), None, false)
        .await
        .expect("failed command panel should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("✗ failed (exit 5)"), "failed badge reads as failure");
    assert!(!inline_output.contains("could not compile"), "failed stderr stays out of the transcript");

    let trail = transcript::snapshot().join("\n");
    assert!(trail.contains("could not compile"), "failed stderr joins the trail");
    transcript::clear();
}

#[tokio::test]
async fn successful_command_keeps_stderr_inline() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "cargo check",
        "exit_code": 0,
        "stderr": "warning: unused import"
    });

    render_terminal_command_panel(&mut renderer, &payload, &GitStyles::new(), &LsStyles::from_env(), None, false)
        .await
        .expect("successful command panel should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("warning: unused import"), "successful stderr is ordinary output");
}

#[tokio::test]
async fn successful_pty_command_badge_stays_a_success() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "cargo check",
        "session_id": "run-2",
        "output": "checking vtcode",
        "is_exited": true,
        "exit_code": 0
    });

    render_terminal_command_panel(&mut renderer, &payload, &GitStyles::new(), &LsStyles::from_env(), None, false)
        .await
        .expect("successful PTY panel should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("✓ exit 0"), "success badge stays a success");
}
