//! Split from streams.rs; see module docs there.

use super::*;

pub(crate) enum HiddenLinesNoticeKind {
    CommandPreview,
    /// Exec-session stdin/stdout overflow. Points at the in-TUI expand
    /// affordance instead of the share-hint copy used for command previews.
    ExecSessionExpand,
    Generic,
    TokenBudget,
}

/// Expand affordance copy for a truncated exec-session body.
///
/// `underline_action` embeds SGR underline on the click target so TUI
/// hit-region detection can treat it as clickable. CLI sinks pass `false`:
/// raw escapes would leak into plain output, and they have no hit regions.
fn exec_session_expand_notice(hidden: usize, underline_action: bool) -> String {
    let summary = shared_hidden_lines_summary(hidden);
    if underline_action {
        let underline = AnsiStyle::new().effects(Effects::UNDERLINE);
        format!("{summary} · {underline}click to expand{Reset}")
    } else {
        format!("{summary} · click to expand")
    }
}

pub(crate) fn hidden_lines_notice(hidden: usize, kind: HiddenLinesNoticeKind) -> String {
    hidden_lines_notice_with(hidden, kind, true)
}

pub(crate) fn hidden_lines_notice_with(hidden: usize, kind: HiddenLinesNoticeKind, underline_expand: bool) -> String {
    match kind {
        HiddenLinesNoticeKind::CommandPreview => {
            format!("    {} (/share html for full transcript)", shared_hidden_lines_summary(hidden))
        }
        HiddenLinesNoticeKind::ExecSessionExpand => exec_session_expand_notice(hidden, underline_expand),
        HiddenLinesNoticeKind::Generic => {
            format!("[... {} line{} truncated ...]", hidden, if hidden == 1 { "" } else { "s" })
        }
        HiddenLinesNoticeKind::TokenBudget => "[... content truncated by token budget ...]".to_string(),
    }
}
