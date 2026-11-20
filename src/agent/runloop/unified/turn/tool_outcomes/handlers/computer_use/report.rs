//! Redacted human- and model-facing report lines for computer turns.
//!
//! Payloads never appear here: per-call verdicts name ids and outcomes only,
//! so screenshots and typed text stay out of transcripts and tool results.

use serde_json::Value;

use super::execution::{ComputerTurnOutcome, ComputerTurnOutcomeKind, NativeTurnResults};

/// Off-state guidance for a Disabled reason, relayed by the model. The tool
/// is advanced-visible while the switch is off, so these lines are the
/// activation CTA: each names the concrete next step. Unknown reasons
/// get the generic setup pointer so a new producer degrades to guidance,
/// never to a bare reason.
fn disabled_off_state_guidance(reason: &str) -> &'static str {
    match reason {
        "disabled" => "Enable it in Settings (Computer Use), configure a preview build target, then retry.",
        "unsupported provider" => "Switch to an OpenAI or Anthropic model, then retry.",
        "no target configured" => "Set the browser build target in Settings (Computer Use), then retry.",
        "external address outside the offline preview" => {
            "Capture it through the shell instead: run headless Chrome via exec_command (e.g. `chrome --headless --screenshot=/tmp/shot.png --window-size=1280,800 --virtual-time-budget=8000 URL`; macOS app binary at /Applications/Google Chrome.app/Contents/MacOS/Google Chrome), or `screencapture -x /tmp/shot.png` for the desktop."
        }
        _ => "Check Settings (Computer Use) for setup, then retry.",
    }
}

pub(crate) fn turn_report_lines(outcome: &ComputerTurnOutcome) -> Vec<String> {
    match &outcome.kind {
        ComputerTurnOutcomeKind::Disabled { reason } => vec![format!(
            "Computer turn did not run: {reason}. {}",
            disabled_off_state_guidance(reason)
        )],
        ComputerTurnOutcomeKind::NoNativeCalls { content, ordinaries } => {
            let mut lines = vec![format!(
                "The model made no computer calls ({} ordinary calls ignored).",
                ordinaries.len()
            )];
            if let Some(text) = content.as_deref().filter(|text| !text.trim().is_empty()) {
                lines.push(format!("Model said: {}", vtcode_commons::formatting::truncate_text(text, 500, "…")));
            }
            lines
        }
        ComputerTurnOutcomeKind::ApprovalDenied => vec!["Computer turn denied.".to_string()],
        ComputerTurnOutcomeKind::BudgetExceeded { proposed, max } => {
            vec![format!(
                "Computer turn refused: {proposed} proposed actions exceed the budget of {max}."
            )]
        }
        ComputerTurnOutcomeKind::ReviewHalted => {
            vec!["Review halted the turn: the batch was judged SUSPECT.".to_string()]
        }
        ComputerTurnOutcomeKind::Cancelled => vec!["Computer turn cancelled.".to_string()],
        ComputerTurnOutcomeKind::Failed { stage, message } => {
            vec![format!("Computer turn failed at the {stage:?} stage: {message}")]
        }
        ComputerTurnOutcomeKind::Completed { results, summary } => {
            let mut lines = vec![format!(
                "Computer turn completed ({}): {} units.",
                outcome.model, summary.executed_units
            )];
            lines.extend(result_report_lines(results));
            lines
        }
    }
}

/// Per-call verdicts without payloads: screenshots never reach the transcript.
fn result_report_lines(results: &NativeTurnResults) -> Vec<String> {
    match results {
        NativeTurnResults::Anthropic(positioned) => positioned
            .iter()
            .map(|(index, block)| {
                let id = block.get("tool_use_id").and_then(Value::as_str).unwrap_or("?");
                let verdict = if block.get("is_error") == Some(&serde_json::json!(true)) {
                    "error"
                } else {
                    "ok"
                };
                format!("call {id} (#{index}): {verdict}")
            })
            .collect(),
        NativeTurnResults::OpenAi(outputs) => outputs
            .iter()
            .map(|output| {
                let id = output.get("call_id").and_then(Value::as_str).unwrap_or("?");
                format!("call {id}: ok")
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::execution::{ComputerFailureStage, RedactedTurnSummary, TeardownOutcome};
    use super::*;

    #[test]
    fn disabled_reports_guide_the_off_state_with_a_next_step() {
        // The tool is advanced-visible while the switch is off, so every
        // Disabled outcome must relay activation, not just name the
        // reason. Unknown reasons degrade to the generic setup pointer.
        let outcome = |reason| ComputerTurnOutcome {
            kind: ComputerTurnOutcomeKind::Disabled { reason },
            model: "test-model".to_string(),
            usage: None,
        };
        for (reason, needle) in [
            ("disabled", "Enable it in Settings (Computer Use)"),
            ("unsupported provider", "Switch to an OpenAI or Anthropic model"),
            ("no target configured", "Set the browser build target in Settings (Computer Use)"),
            ("external address outside the offline preview", "Capture it through the shell instead"),
            ("some future reason", "Check Settings (Computer Use) for setup"),
        ] {
            let lines = turn_report_lines(&outcome(reason));
            assert_eq!(lines.len(), 1, "{reason}");
            assert!(lines[0].contains(&format!("did not run: {reason}.")));
            assert!(lines[0].contains(needle), "{reason}: {}", lines[0]);
        }
    }

    #[test]
    fn computer_use_turn_reports_name_every_outcome_without_payloads() {
        let summary = RedactedTurnSummary {
            target_kind: "browser",
            target: "fixture-app".to_string(),
            recipient: "test-model (test)".to_string(),
            action_kinds: vec!["screenshot".to_string()],
            action_count: 1,
            executed_units: 1,
            review: None,
            teardown_trigger: "finished",
            teardown: TeardownOutcome::Removed,
        };
        let ok_block = serde_json::json!({"type": "tool_result", "tool_use_id": "c1", "is_error": false});
        let err_block =
            serde_json::json!({"type": "tool_result", "tool_use_id": "c2", "is_error": true, "content": "secret"});
        let shot = serde_json::json!({"type": "computer_call_output", "call_id": "call_1", "output": {"image_url": "data:image/png;base64,SECRET"}});
        let outcome = |kind| ComputerTurnOutcome { kind, model: "test-model".to_string(), usage: None };
        for (kind, needle) in [
            (ComputerTurnOutcomeKind::Disabled { reason: "disabled" }, "did not run: disabled."),
            (ComputerTurnOutcomeKind::NoNativeCalls { content: None, ordinaries: vec![] }, "no computer calls"),
            (ComputerTurnOutcomeKind::ApprovalDenied, "denied."),
            (
                ComputerTurnOutcomeKind::BudgetExceeded { proposed: 60, max: 50 },
                "60 proposed actions exceed the budget of 50",
            ),
            (ComputerTurnOutcomeKind::ReviewHalted, "SUSPECT"),
            (ComputerTurnOutcomeKind::Cancelled, "cancelled."),
            (
                ComputerTurnOutcomeKind::Failed {
                    stage: ComputerFailureStage::Execute,
                    message: "boom".to_string(),
                },
                "failed at the Execute stage: boom",
            ),
        ] {
            let lines = turn_report_lines(&outcome(kind));
            assert!(lines.iter().any(|line| line.contains(needle)), "{lines:?} missing {needle}");
        }
        let lines = turn_report_lines(&outcome(ComputerTurnOutcomeKind::NoNativeCalls {
            content: Some("just chatting".to_string()),
            ordinaries: vec![],
        }));
        assert!(lines.iter().any(|line| line == "Model said: just chatting"), "{lines:?}");
        let lines = turn_report_lines(&outcome(ComputerTurnOutcomeKind::Completed {
            results: NativeTurnResults::Anthropic(vec![(1, ok_block), (3, err_block)]),
            summary: summary.clone(),
        }));
        assert!(lines[0].contains("completed (test-model): 1 units"), "{lines:?}");
        assert!(lines.iter().any(|line| line == "call c1 (#1): ok"), "{lines:?}");
        assert!(lines.iter().any(|line| line == "call c2 (#3): error"), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("secret")), "{lines:?}");
        let lines = turn_report_lines(&outcome(ComputerTurnOutcomeKind::Completed {
            results: NativeTurnResults::OpenAi(vec![shot]),
            summary,
        }));
        assert!(lines.iter().any(|line| line == "call call_1: ok"), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("SECRET")), "{lines:?}");
    }
}
