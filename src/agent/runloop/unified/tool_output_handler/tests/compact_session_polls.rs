use super::*;

fn wait_args(session: &str, timeout_secs: u64) -> serde_json::Value {
    serde_json::json!({
        "session_id": session,
        "action": "wait",
        "wait_timeout_seconds": timeout_secs,
    })
}

fn wait_output(session: &str, waited_secs: f64) -> serde_json::Value {
    serde_json::json!({
        "output": "tail preview",
        "session_id": session,
        "waited_seconds": waited_secs,
        "next_continue_args": { "session_id": session },
    })
}

fn visible_text(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<InlineCommand>) -> String {
    std::iter::from_fn(|| receiver.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendLine { segments, .. } => {
                Some(segments.iter().map(|segment| segment.text.as_str()).collect::<String>())
            }
            InlineCommand::AppendToolOutputLine { segments, .. } => {
                Some(segments.iter().map(|segment| segment.text.as_str()).collect::<String>())
            }
            InlineCommand::Inline { segment, .. } => Some(segment.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn render_wait(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    session: &str,
    timeout_secs: u64,
    waited_secs: f64,
) {
    render_tool_output_common(
        renderer,
        handle,
        tools::WRITE_STDIN,
        &wait_args(session, timeout_secs),
        &wait_output(session, waited_secs),
        true,
        None,
        None,
    )
    .await
    .expect("wait output should render");
}

#[tokio::test]
async fn consecutive_session_waits_share_one_cli_header() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    render_wait(&mut renderer, &handle, "run-1", 1, 1.0).await;
    render_wait(&mut renderer, &handle, "run-1", 2, 2.0).await;
    render_wait(&mut renderer, &handle, "run-1", 4, 4.0).await;

    let text = visible_text(&mut receiver);
    assert_eq!(
        text.matches("Wait for command session").count(),
        1,
        "three consecutive waits share one header:\n{text}"
    );
    assert_eq!(text.matches("Session run-1").count(), 1, "the session detail line renders once:\n{text}");
    assert_eq!(text.matches('×').count(), 2, "each follow-up ticks once:\n{text}");
    assert!(text.contains("×2 · waited 3s"), "the second tick totals waits so far (1s + 2s):\n{text}");
    assert!(text.contains("×3 · waited 7s"), "the third tick totals waits so far (1s + 2s + 4s):\n{text}");
}

#[tokio::test]
async fn repeated_poll_hint_renders_once_per_run() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    render_wait(&mut renderer, &handle, "run-1", 1, 1.0).await;
    render_wait(&mut renderer, &handle, "run-1", 2, 2.0).await;

    let text = visible_text(&mut receiver);
    assert_eq!(
        text.matches("Still running — check again for more output.").count(),
        1,
        "the identical follow-up hint renders once:\n{text}"
    );
}

#[tokio::test]
async fn different_session_starts_new_cli_header() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    render_wait(&mut renderer, &handle, "run-1", 1, 1.0).await;
    render_wait(&mut renderer, &handle, "run-2", 1, 1.0).await;
    render_wait(&mut renderer, &handle, "run-1", 1, 1.0).await;

    let text = visible_text(&mut receiver);
    assert_eq!(text.matches("Wait for command session").count(), 3, "non-consecutive sessions never merge:\n{text}");
}

#[tokio::test]
async fn interleaved_command_breaks_cli_poll_run() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    render_wait(&mut renderer, &handle, "run-1", 1, 1.0).await;
    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "printf one"}),
        &serde_json::json!({"stdout": "one"}),
        true,
        None,
        None,
    )
    .await
    .expect("command output should render");
    render_wait(&mut renderer, &handle, "run-1", 2, 2.0).await;

    let text = visible_text(&mut receiver);
    assert_eq!(
        text.matches("Wait for command session").count(),
        2,
        "an intervening command ends coalescing:\n{text}"
    );
}

#[tokio::test]
async fn failed_wait_keeps_its_cli_header_and_breaks_the_run() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    render_wait(&mut renderer, &handle, "run-1", 1, 1.0).await;
    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::WRITE_STDIN,
        &wait_args("run-1", 2),
        &serde_json::json!({"error": "session lost"}),
        false,
        None,
        None,
    )
    .await
    .expect("failed wait output should render");
    render_wait(&mut renderer, &handle, "run-1", 4, 4.0).await;

    let text = visible_text(&mut receiver);
    assert_eq!(
        text.matches("Wait for command session").count(),
        3,
        "the failure keeps its header and starts a fresh run:\n{text}"
    );
}

#[tokio::test]
async fn expanded_session_waits_share_one_cli_header() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Expanded);

    render_wait(&mut renderer, &handle, "run-1", 1, 1.0).await;
    render_wait(&mut renderer, &handle, "run-1", 2, 2.0).await;

    let text = visible_text(&mut receiver);
    assert_eq!(
        text.matches("Wait for command session").count(),
        1,
        "expanded mode coalesces like compact mode:\n{text}"
    );
}
