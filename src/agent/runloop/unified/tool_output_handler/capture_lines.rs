//! Complete-capture loading and merged command-output line building.

use super::*;

pub(super) async fn load_complete_output(output: &serde_json::Value, workspace_root: Option<&Path>) -> Option<String> {
    // A present but malformed spool marker is still a spool reference. Never
    // reinterpret its inline preview as a trustworthy complete capture.
    if output.get("spool_path").is_some() && vtcode_core::tools::SpooledOutputReference::from_value(output).is_none() {
        return None;
    }
    if let Some(reference) = vtcode_core::tools::SpooledOutputReference::from_value(output) {
        let root = workspace_root?;
        let owned = reference.spool_path.to_string();
        let root = root.to_path_buf();
        let state = reference.state;
        let byte_count = reference.original_bytes;
        let digest = reference.sha256.map(str::to_string);
        return tokio::task::spawn_blocking(move || {
            let value = serde_json::json!({
                "spool_path": owned,
                "spooled_bytes": byte_count,
                "spool_sha256": digest,
                "spool_state": match state {
                    vtcode_core::tools::SpoolState::Pending => "pending",
                    vtcode_core::tools::SpoolState::Completed => "completed",
                    vtcode_core::tools::SpoolState::Unknown => "unknown",
                },
            });
            let reference = vtcode_core::tools::SpooledOutputReference::from_value(&value)?;
            match state {
                vtcode_core::tools::SpoolState::Completed => reference.read_verified_completed(&root).ok(),
                vtcode_core::tools::SpoolState::Pending => reference.read_pending_bounded(&root, 64 * 1024).ok(),
                vtcode_core::tools::SpoolState::Unknown => None,
            }
        })
        .await
        .ok()
        .flatten();
    }

    if output_text(output, "output").is_none()
        && (output_text(output, "stdout").is_some() || output_text(output, "stderr").is_some())
    {
        // Named pipe streams remain labeled in the viewer. Joining them here
        // would make the later capture renderer mistake stderr for a copy of
        // stdout and drop it.
        return None;
    }

    let texts = ordered_stream_texts(output);
    (!texts.is_empty()).then(|| texts.join("\n"))
}

pub(super) fn normalize_terminal_output_lines(capture: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut chars = capture.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => match chars.next() {
                Some('[') => {
                    let mut params = String::new();
                    let final_byte = loop {
                        let Some(next) = chars.next() else {
                            break None;
                        };
                        if ('@'..='~').contains(&next) {
                            break Some(next);
                        }
                        params.push(next);
                    };

                    match final_byte {
                        // Clear-screen sequences mean that the earlier text
                        // was only a stale terminal frame, not command output
                        // that should remain in the readable viewer.
                        Some('J') if params.starts_with('2') || params.starts_with('3') => {
                            lines.clear();
                            current.clear();
                        }
                        // Erase the current line for the common progress-bar
                        // rewrite sequence. Styling and cursor movement are
                        // intentionally omitted from the plain-text viewer.
                        Some('K') if params.starts_with('2') => current.clear(),
                        _ => {}
                    }
                }
                Some(']') => {
                    // Skip OSC title/hyperlink sequences through BEL or ST.
                    while let Some(next) = chars.next() {
                        if next == '\x07' {
                            break;
                        }
                        if next == '\x1b' && chars.peek() == Some(&'\\') {
                            let _ = chars.next();
                            break;
                        }
                    }
                }
                Some(_) | None => {}
            },
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    let _ = chars.next();
                    lines.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
            }
            '\n' => lines.push(std::mem::take(&mut current)),
            '\u{8}' => {
                let _ = current.pop();
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

pub(super) fn normalized_lines_contain_subsequence(container: &[String], candidate: &[String]) -> bool {
    if candidate.is_empty() {
        return false;
    }

    let mut candidate_index = 0;
    for line in container {
        if line == &candidate[candidate_index] {
            candidate_index += 1;
            if candidate_index == candidate.len() {
                return true;
            }
        }
    }
    false
}

pub(super) fn command_output_header(name: &str, args: &serde_json::Value, workspace_root: Option<&Path>) -> String {
    // Transcript Review shows the complete command: viewport-aware wrapping
    // (TUI reflow) owns overflow instead of a `…` truncation, so long chained
    // commands remain readable in full. Uses the same script-runner folding as
    // compact previews but applies no length cap.
    display_command_text(args)
        .map(|command| relativize_command_paths(&command, workspace_root))
        .map(|command| preview_full_command(&command))
        .filter(|command| !command.is_empty())
        .map(|command| format!("• Ran {command}"))
        .unwrap_or_else(|| format!("• Ran {name}"))
}

pub(super) fn append_merged_output_lines(lines: &mut Vec<String>, output_lines: impl IntoIterator<Item = String>) {
    for (index, line) in output_lines.into_iter().enumerate() {
        if index == 0 {
            lines.push(format!("  └ {line}"));
        } else {
            lines.push(format!("    {line}"));
        }
    }
}

pub(super) fn append_labeled_output_lines(
    lines: &mut Vec<String>,
    label: &str,
    output_lines: impl IntoIterator<Item = String>,
) {
    lines.push(format!("  {label}:"));
    for line in output_lines {
        lines.push(format!("    {line}"));
    }
}

pub(super) fn append_viewer_status_line(
    lines: &mut Vec<String>,
    output: &serde_json::Value,
    status: ToolDisplayStatus,
) {
    if !matches!(status, ToolDisplayStatus::Success)
        && let Some(completion) = compact_run_completion_line(output, status)
    {
        lines.push(format!("    {completion}"));
    }
}

pub(super) fn build_merged_command_output_lines(
    name: &str,
    args: &serde_json::Value,
    capture: &str,
    workspace_root: Option<&Path>,
    output: &serde_json::Value,
    status: ToolDisplayStatus,
) -> Vec<String> {
    let mut lines = vec![command_output_header(name, args, workspace_root)];
    let named_streams = canonical_pipe_streams(output);
    let capture_lines = normalize_terminal_output_lines(capture);
    let has_spool_metadata = output.get("spool_path").is_some();

    if has_spool_metadata {
        // A successfully loaded spool is the complete capture; the inline
        // output field is only a bounded preview and must not be shown beside
        // it. If the spool could not be loaded, keep the path fail-closed and
        // avoid presenting the untrusted preview as complete output.
        if !capture_lines.is_empty() {
            append_merged_output_lines(&mut lines, capture_lines.clone());
            for stream in &named_streams {
                let Some(label) = stream.label else {
                    continue;
                };
                let stream_lines = normalize_terminal_output_lines(stream.text);
                if !normalized_lines_contain_subsequence(&capture_lines, &stream_lines) {
                    append_labeled_output_lines(&mut lines, label, stream_lines);
                }
            }
        }
    } else if named_streams.is_empty() {
        append_merged_output_lines(&mut lines, capture_lines);
    } else {
        for stream in &named_streams {
            let output_lines = normalize_terminal_output_lines(stream.text);
            if let Some(label) = stream.label {
                append_labeled_output_lines(&mut lines, label, output_lines);
            } else {
                append_merged_output_lines(&mut lines, output_lines);
            }
        }

        // A PTY spool can contain terminal data not represented by the named
        // pipe fields. Preserve that extra capture explicitly, but do not use
        // it to deduplicate stdout and stderr when no merged field is present.
        let named_lines = named_streams
            .iter()
            .flat_map(|stream| normalize_terminal_output_lines(stream.text))
            .collect::<Vec<_>>();
        if !capture_lines.is_empty() && capture_lines != named_lines {
            append_labeled_output_lines(&mut lines, "output", capture_lines);
        }
    }
    if let Some(note) = output_text(output, "critical_note") {
        lines.push(format!("    {note}"));
    }
    append_warning_line(&mut lines, output);
    append_viewer_status_line(&mut lines, output, status);
    append_structured_command_context(&mut lines, output);
    lines
}

pub(super) fn build_pipe_command_output_lines(
    name: &str,
    args: &serde_json::Value,
    output: &serde_json::Value,
    workspace_root: Option<&Path>,
    status: ToolDisplayStatus,
) -> Vec<String> {
    let mut lines = vec![command_output_header(name, args, workspace_root)];
    for stream in canonical_pipe_streams(output) {
        let output_lines = normalize_terminal_output_lines(stream.text);
        if output_lines.is_empty() {
            continue;
        }
        if let Some(label) = stream.label {
            append_labeled_output_lines(&mut lines, label, output_lines);
        } else {
            append_merged_output_lines(&mut lines, output_lines);
        }
    }
    if let Some(note) = output_text(output, "critical_note") {
        lines.push(format!("    {note}"));
    }
    append_warning_line(&mut lines, output);
    append_viewer_status_line(&mut lines, output, status);
    append_structured_command_context(&mut lines, output);
    lines
}

pub(super) fn append_follow_up_capture_lines(
    lines: &mut Vec<String>,
    output: &serde_json::Value,
    rendered_output: Option<&str>,
) {
    for hint in crate::agent::runloop::tool_output::tool_follow_up_hints_for_capture(output, rendered_output) {
        lines.push(format!("    {hint}"));
    }
}
