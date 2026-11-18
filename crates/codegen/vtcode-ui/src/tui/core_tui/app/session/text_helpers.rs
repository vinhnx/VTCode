//! Transcript text helpers used by the session's tool-output review paths.
//!
//! These strip ANSI styling and fold continued PTY header lines into a single
//! comparable string. Grouped here so `session/mod.rs` stays focused on the
//! `AppSession` state and its driver impls; the helpers used outside this module
//! are re-imported into the parent, preserving every original call path.

use super::*;

pub(super) fn normalize_tool_output_header(header: &str) -> String {
    crate::tui::core_tui::session::text_utils::strip_ansi_codes(header)
        .trim_end()
        .to_owned()
}

fn rendered_core_line_text(core: &CoreSessionState, line_index: usize) -> String {
    let Some(line) = core.lines.get(line_index) else {
        return String::new();
    };
    core.render_message_spans_for_line(line)
        .into_iter()
        .map(|span| crate::tui::core_tui::session::text_utils::strip_ansi_codes(span.content.as_ref()).into_owned())
        .collect()
}

pub(super) fn folded_pty_header(core: &CoreSessionState, line_index: usize) -> String {
    let first = rendered_core_line_text(core, line_index);
    if !first.trim_start().starts_with('•') {
        return first;
    }

    let mut folded = first.trim_end().to_owned();
    let mut continuation_index = line_index + 1;
    while let Some(line) = core.lines.get(continuation_index) {
        if line.kind != InlineMessageKind::Pty {
            break;
        }
        let continuation = rendered_core_line_text(core, continuation_index);
        let Some(fragment) = continuation.trim_start().strip_prefix('│') else {
            break;
        };
        folded.push(' ');
        folded.push_str(fragment.trim());
        continuation_index += 1;
    }
    folded
}
