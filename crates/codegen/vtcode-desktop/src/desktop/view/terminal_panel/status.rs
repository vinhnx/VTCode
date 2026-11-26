//! Header program-status pill mapping, split for the 1000-line ceiling.

use crate::desktop::terminal_program_status::TerminalProgramStatusSummary;
use gpui_kit::assets::IconName;
use rio_vt::event::ProgramState;

/// Header status glyph per program state: distinct shapes carry the meaning,
/// never color alone.
pub(super) fn terminal_status_icon(state: ProgramState) -> IconName {
    match state {
        ProgramState::Working => IconName::Activity,
        ProgramState::Blocked => IconName::TriangleAlert,
        ProgramState::Done => IconName::Check,
        // The summary never surfaces idle (quiet header) and clear is never
        // stored; fail closed on the error glyph so an impossible state still
        // reads as needing attention.
        ProgramState::Error | ProgramState::Idle | ProgramState::Clear => IconName::CircleX,
    }
}

/// Header status tone per program state, from theme tokens by semantic role.
pub(super) fn terminal_status_tone(
    state: ProgramState,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> gpui_kit::Hsla {
    match state {
        ProgramState::Working => colors.info,
        ProgramState::Blocked => colors.warning,
        ProgramState::Done => colors.success,
        ProgramState::Error | ProgramState::Idle | ProgramState::Clear => colors.danger,
    }
}

/// Header status text: the record's sanitized detail, or a localized state
/// label when it carries no text, with a progress suffix when reported.
pub(super) fn terminal_status_label(summary: &TerminalProgramStatusSummary) -> String {
    let base = if summary.detail.is_empty() {
        let key = match summary.state {
            ProgramState::Working => "app.navigation.terminal_status_working",
            ProgramState::Blocked => "app.navigation.terminal_status_blocked",
            ProgramState::Done => "app.navigation.terminal_status_done",
            ProgramState::Error | ProgramState::Idle | ProgramState::Clear => "app.navigation.terminal_status_error",
        };
        rust_i18n::t!(key).to_string()
    } else {
        summary.detail.clone()
    };
    match summary.progress {
        Some(percent) => format!("{base} · {percent}%"),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::terminal_program_status::TerminalProgramStatusSummary;
    use gpui_kit::assets::IconName;
    use rio_vt::event::ProgramState;

    #[test]
    fn terminal_status_icon_uses_distinct_shapes_per_state() {
        assert!(matches!(terminal_status_icon(ProgramState::Working), IconName::Activity));
        assert!(matches!(terminal_status_icon(ProgramState::Blocked), IconName::TriangleAlert));
        assert!(matches!(terminal_status_icon(ProgramState::Done), IconName::Check));
        // Error plus the two impossible states fail closed on one glyph.
        for state in [ProgramState::Error, ProgramState::Idle, ProgramState::Clear] {
            assert!(matches!(terminal_status_icon(state), IconName::CircleX), "{state:?}");
        }
    }

    #[test]
    fn terminal_status_tone_follows_semantic_theme_roles() {
        use gpui_kit::component::theme::ThemeColor;
        let colors = ThemeColor::default();
        assert_eq!(terminal_status_tone(ProgramState::Working, &colors), colors.info);
        assert_eq!(terminal_status_tone(ProgramState::Blocked, &colors), colors.warning);
        assert_eq!(terminal_status_tone(ProgramState::Done, &colors), colors.success);
        for state in [ProgramState::Error, ProgramState::Idle, ProgramState::Clear] {
            assert_eq!(terminal_status_tone(state, &colors), colors.danger, "{state:?}");
        }
    }

    #[test]
    fn terminal_status_label_uses_detail_or_state_and_keeps_progress() {
        let summary = |state, detail: &str, progress| TerminalProgramStatusSummary {
            state,
            detail: detail.to_string(),
            progress,
        };
        assert_eq!(
            terminal_status_label(&summary(ProgramState::Working, "Pushing image", Some(40))),
            "Pushing image · 40%"
        );
        assert_eq!(terminal_status_label(&summary(ProgramState::Blocked, "Approve deploy?", None)), "Approve deploy?");
        assert_eq!(terminal_status_label(&summary(ProgramState::Working, "", None)), "Working");
        assert_eq!(terminal_status_label(&summary(ProgramState::Working, "", Some(40))), "Working · 40%");
        assert_eq!(terminal_status_label(&summary(ProgramState::Blocked, "", None)), "Needs input");
        assert_eq!(terminal_status_label(&summary(ProgramState::Done, "", None)), "Done");
        assert_eq!(terminal_status_label(&summary(ProgramState::Error, "", None)), "Failed");
    }
}
