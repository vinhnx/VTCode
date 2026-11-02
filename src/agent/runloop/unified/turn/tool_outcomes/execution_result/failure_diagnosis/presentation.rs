//! Tool-response attachment and user-facing diagnosis emission.

use serde_json::json;
use vtcode_core::utils::ansi::MessageStyle;

use super::ToolFailureDiagnosis;
use super::evidence::bounded_field;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

pub(super) fn attach_to_serialized_tool_response(serialized: String, diagnosis: &ToolFailureDiagnosis) -> String {
    let mut payload =
        serde_json::from_str::<serde_json::Value>(&serialized).unwrap_or_else(|_| json!({ "output": serialized }));
    if let Some(object) = payload.as_object_mut() {
        object.insert("diagnosis".to_string(), diagnosis.to_value());
    } else {
        payload = json!({ "output": payload, "diagnosis": diagnosis.to_value() });
    }
    serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) async fn push_tool_response_with_diagnosis(
    t_ctx: &mut super::super::super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    content_for_model: String,
    diagnosis: &ToolFailureDiagnosis,
    batch_spinner: Option<&crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner>,
) -> anyhow::Result<()> {
    let diagnosed_content = attach_to_serialized_tool_response(content_for_model, diagnosis);
    super::super::auto_permission_probe::push_tool_response_with_auto_permission_probe(
        t_ctx,
        tool_call_id,
        tool_name,
        diagnosed_content,
        batch_spinner,
    )
    .await
}

fn concise_diagnosis_line(tool_name: &str, diagnosis: &ToolFailureDiagnosis) -> String {
    let display_tool_name = bounded_field(tool_name);
    if diagnosis.no_matches {
        return format!("Tool '{display_tool_name}': no matching results (exit code 1).");
    }
    let observed_headline = diagnosis
        .observed
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("failed");
    let observed_headline: String = if observed_headline.chars().count() > 160 {
        format!("{}…", observed_headline.chars().take(159).collect::<String>())
    } else {
        observed_headline.to_string()
    };
    format!("Tool '{display_tool_name}' failed: {observed_headline}")
}

fn diagnostic_correlation_id(
    turn_id: &str,
    tool_call_id: Option<&str>,
    tool_name: &str,
    diagnosis: &ToolFailureDiagnosis,
) -> String {
    use std::hash::{Hash, Hasher};

    let mut identity = std::collections::hash_map::DefaultHasher::new();
    if let Some(tool_call_id) = tool_call_id.filter(|id| !id.trim().is_empty()) {
        (turn_id, tool_name, tool_call_id, &diagnosis.observed).hash(&mut identity);
    } else {
        // Preserve the legacy identity for callers that do not have a provider
        // call ID, so existing persisted diagnostics remain update-compatible.
        (turn_id, tool_name, &diagnosis.observed).hash(&mut identity);
    }
    format!("tool-diagnosis:{:016x}", identity.finish())
}

pub(crate) fn render_diagnosis(
    renderer: &mut vtcode_core::utils::ansi::AnsiRenderer,
    _harness_emitter: Option<&crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    turn_id: &str,
    tool_call_id: Option<&str>,
    tool_name: &str,
    diagnosis: &ToolFailureDiagnosis,
) {
    use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};
    // Empty search results are ordinary tool outcomes, not operational failures.
    if diagnosis.no_matches {
        if let Err(error) = renderer.line(MessageStyle::Info, &concise_diagnosis_line(tool_name, diagnosis)) {
            tracing::warn!(error = %error, "failed to render empty search result");
        }
        return;
    }
    let diagnostic = Diagnostic {
        severity: DiagnosticSeverity::Warning,
        code: "tool.execution_failed".into(),
        summary: format!("Tool '{}' failed. Expand Details for recovery steps.", bounded_field(tool_name)),
        agent_details: diagnosis.render_text(tool_name),
        correlation_id: diagnostic_correlation_id(turn_id, tool_call_id, tool_name, diagnosis),
        actions: Vec::new(),
    };
    // The renderer's canonical callback owns the event; do not emit a second incident.
    if let Err(error) = renderer.diagnostic(&diagnostic) {
        tracing::warn!(tool = %tool_name, error = %error, "failed to persist or render tool failure diagnosis");
    }
}

pub(crate) fn render_and_emit(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: Option<&str>,
    tool_name: &str,
    diagnosis: &ToolFailureDiagnosis,
) {
    render_diagnosis(
        ctx.renderer,
        ctx.harness_emitter,
        &ctx.harness_state.turn_id.0,
        tool_call_id,
        tool_name,
        diagnosis,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concise_line_uses_headline_and_stays_single_line() {
        // `bounded_field` flattens newlines to spaces, so multi-line input
        // arrives single-line; TUI must still stay single-line without leaking
        // the verbose Likely-cause/Next-action fields or old header.
        let multi = ToolFailureDiagnosis::new("first line\nsecond line with detail\nthird line", "cause", "next");
        let line = concise_diagnosis_line("request_user_input", &multi);
        assert!(line.starts_with("Tool 'request_user_input' failed: first line"));
        assert!(!line.contains("Likely cause"), "no verbose fields leak: {line}");
        assert!(!line.contains("Next action"), "no verbose fields leak: {line}");
        assert!(!line.contains('\n'), "TUI stays single-line: {line}");
        assert!(!line.contains("Diagnosis:"), "old verbose header gone: {line}");
    }

    #[test]
    fn concise_search_diagnosis_distinguishes_no_matches_from_errors() {
        use serde_json::json;
        let args = json!({"cmd":"grep absent sample.txt"});
        let empty = super::super::deterministic_output_diagnosis("exec_command", &args, &json!({"exit_code":1}));
        assert_eq!(
            concise_diagnosis_line("exec_command", &empty),
            "Tool 'exec_command': no matching results (exit code 1)."
        );
        for output in [
            json!({"exit_code":2}),
            json!({"exit_code":1,"stderr":"invalid argument"}),
            json!({"exit_code":1,"output_truncated":true}),
        ] {
            let failed = super::super::deterministic_output_diagnosis("exec_command", &args, &output);
            assert!(concise_diagnosis_line("exec_command", &failed).contains(" failed: "));
        }
        let model_text = ToolFailureDiagnosis::new("no matches", "no matches", "try another query");
        assert!(concise_diagnosis_line("exec_command", &model_text).contains(" failed: "));
        assert!(empty.render_text("exec_command").contains("exit code 1"));
    }

    #[test]
    fn aborted_grep_chain_hint_names_unrun_checks() {
        use serde_json::json;
        // Session-891943 events 618-619: first grep no-match aborts `&&`.
        let args = json!({"cmd":"grep -n foo README.md && grep -n bar README.md"});
        let out = super::super::deterministic_output_diagnosis("exec_command", &args, &json!({"exit_code":1}));
        assert!(out.next_action.contains("aborts `&&` chains"), "chain hint: {}", out.next_action);
        // Standalone grep no-match keeps its existing empty-result guidance.
        let single = super::super::deterministic_output_diagnosis(
            "exec_command",
            &json!({"cmd":"grep absent sample.txt"}),
            &json!({"exit_code":1}),
        );
        assert!(!single.next_action.contains("aborts `&&` chains"));
        // Real errors keep generic guidance.
        let err = super::super::deterministic_output_diagnosis(
            "exec_command",
            &args,
            &json!({"exit_code":1,"stderr":"invalid argument"}),
        );
        assert!(!err.next_action.contains("aborts `&&` chains"));
    }

    #[test]
    fn concise_line_bounds_long_observed_asymmetrically() {
        let short = ToolFailureDiagnosis::new("exit 1", "cause", "next");
        let short_line = concise_diagnosis_line("exec_command", &short);
        assert!(short_line.contains("exit 1"));
        assert!(short_line.chars().count() < 120, "short stays short: {short_line}");

        let long_observed = "x".repeat(500);
        let long = ToolFailureDiagnosis::new(long_observed, "cause", "next");
        let long_line = concise_diagnosis_line("exec_command", &long);
        assert!(long_line.chars().count() <= "Tool 'exec_command' failed: ".len() + 160);
        assert!(!long_line.contains("Likely cause"), "no verbose fields leak: {long_line}");
    }

    #[test]
    fn diagnostic_identity_separates_concurrent_tool_calls() {
        let first = ToolFailureDiagnosis::new("same failure", "same cause", "same action");
        let second = first.clone();

        let first_id = diagnostic_correlation_id("turn-1", Some("call-a"), "web_fetch", &first);
        let second_id = diagnostic_correlation_id("turn-1", Some("call-b"), "web_fetch", &second);
        assert_ne!(first_id, second_id, "distinct tool calls must retain distinct diagnostics");

        let repeated_id = diagnostic_correlation_id("turn-1", Some("call-a"), "web_fetch", &second);
        assert_eq!(first_id, repeated_id, "updates for one tool call must replace its own diagnostic");
    }

    #[test]
    fn diagnostic_identity_preserves_legacy_no_call_id_shape() {
        let diagnosis = ToolFailureDiagnosis::new("same failure", "same cause", "same action");
        let missing_id = diagnostic_correlation_id("turn-1", None, "web_fetch", &diagnosis);
        let blank_id = diagnostic_correlation_id("turn-1", Some("  "), "web_fetch", &diagnosis);
        assert_eq!(missing_id, blank_id, "blank IDs must use the legacy identity");
    }
}
