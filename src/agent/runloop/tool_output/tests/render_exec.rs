use super::*;

#[tokio::test]
async fn render_tool_output_exec_command_renders_terminal_panel_with_output() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    renderer.set_tool_display_mode(ToolDisplayMode::Expanded);
    let payload = json!({
        "command": "cargo check",
        "output": "Compiling vtcode v0.135.9",
        "stdout": "Compiling vtcode v0.135.9",
        "stderr": "",
        "is_exited": true,
        "exit_code": 0
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::EXEC_COMMAND), &payload, None)
        .await
        .expect("exec_command payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(
        inline_output.contains("Compiling vtcode v0.135.9"),
        "exec_command output must be rendered in the terminal panel, got: {inline_output}"
    );
    assert!(
        !inline_output.contains("(no output)"),
        "exec_command output must not fall through to the no-output status renderer"
    );
}
#[tokio::test]
async fn render_tool_output_exec_command_compact_hides_completed_stdout() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "cargo check",
        "stdout": "verbose completed output",
        "stderr": ""
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::EXEC_COMMAND), &payload, None)
        .await
        .expect("exec_command payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(!inline_output.contains("verbose completed output"));
}
#[tokio::test]
async fn render_tool_output_exec_pty_cmd_renders_terminal_panel_with_output() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "ls -la",
        "output": "total 0",
        "session_id": "run-456",
        "is_exited": true,
        "exit_code": 0
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::EXEC_PTY_CMD), &payload, None)
        .await
        .expect("exec_pty_cmd payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(
        inline_output.contains("total") && inline_output.contains("✓ exit 0"),
        "exec_pty_cmd output must be rendered in the terminal panel, got: {inline_output}"
    );
}
#[tokio::test]
async fn render_tool_output_run_pty_completed_spooled_output_is_reference_only() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "command": "cargo check",
        "output": "preview text that should not render inline",
        "session_id": "run-123",
        "is_exited": true,
        "exit_code": 0,
        "spool_path": ".vtcode/context/tool_outputs/run-123.txt"
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::RUN_PTY_CMD), &payload, None)
        .await
        .expect("spooled PTY payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("✓ exit 0"));
    assert!(inline_output.contains("Large output was spooled to"));
    assert!(!inline_output.contains("preview text that should not render inline"));
    assert!(!inline_output.contains("(no output)"));
}
#[tokio::test]
async fn render_tool_output_read_file_renders_spool_hint_on_early_return_path() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "path": "README.md",
        "content": "preview",
        "spool_path": ".vtcode/context/tool_outputs/readme.txt"
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::READ_FILE), &payload, None)
        .await
        .expect("read_file payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("Large output was spooled to"));
    assert!(inline_output.contains("exec_command"));
    assert!(inline_output.contains("cat, sed, or rg"));
    assert!(!inline_output.contains("read_file/grep_file"));
}
#[tokio::test]
async fn render_tool_output_web_fetch_content_fallback_renders_follow_up_hint() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
    let payload = json!({
        "content": "preview",
        "spool_path": ".vtcode/context/tool_outputs/web.txt"
    });

    render_tool_output(&mut renderer, Some(vtcode_core::config::constants::tools::WEB_FETCH), &payload, None)
        .await
        .expect("web_fetch payload should render");

    let inline_output = collect_inline_output(&mut receiver);
    assert!(inline_output.contains("Large output was spooled to"));
    assert!(inline_output.contains("exec_command"));
    assert!(inline_output.contains("cat, sed, or rg"));
    assert!(!inline_output.contains("read_file/grep_file"));
}
