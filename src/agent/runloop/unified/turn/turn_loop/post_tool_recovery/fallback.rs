use super::*;

/// Plan-mode recovery fallback. When the tool-free synthesis fails, the
/// "salvaged" text is usually just a rambling recovery monologue with tool-call
/// markup stripped out — not a plan. Injecting that as the plan the user sees is
/// worse than the structured plan-mode message. Keep salvage only when its
/// extracted artifact passes the same readiness gate used by approval.
pub(super) fn plan_mode_recovery_fallback(
    salvaged_text: Option<String>,
    structured_message: &str,
    working_history: &[uni::Message],
) -> String {
    if let Some(text) = salvaged_text
        && let Some(clean) = ready_plan_text(&text)
    {
        // Return the validated extracted plan text (not the raw salvage) so
        // surrounding recovery prose or blank-line framing is never injected
        // as the user-visible plan. `ready_plan_text` already enforces the
        // same readiness gate used by approval.
        clean
    } else {
        build_recovery_fallback(working_history, structured_message)
    }
}

/// Build the deterministic recovery fallback, optionally appending the list of
/// files already read this turn so the next turn can reuse them instead of
/// re-exploring. `lead_in` is the provider-agnostic message shown first.
pub(super) fn build_recovery_fallback(working_history: &[uni::Message], lead_in: &str) -> String {
    let files_read = gather_files_read_this_turn(working_history);
    if files_read.is_empty() {
        lead_in.to_string()
    } else {
        format!(
            "{lead_in}\n\nFiles already read this turn (their content is in the history above):\n{}",
            files_read.iter().map(|f| format!("  - {f}")).collect::<Vec<_>>().join("\n")
        )
    }
}
