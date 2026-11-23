//! Redacted human- and model-facing report lines for computer turns.
//!
//! Payloads never appear here: per-call verdicts name ids and outcomes only,
//! so screenshots and typed text stay out of transcripts and tool results.

use serde_json::Value;

use super::execution::{
    ComputerTurnOutcome, ComputerTurnOutcomeKind, NativeTurnResults, RedactedTurnSummary, TeardownOutcome,
};
use super::review::BatchReviewVerdict;

/// Shell-fallback recipe for an external-scope refusal, selected for the OS
/// the turn runs on. Chrome flags are identical everywhere; only binary
/// locations and desktop tools differ. Every branch prohibits
/// `--no-sandbox`: the model cargo-cults it from container lore and the
/// permission review blocks sandbox-disabling flags. macOS is verified
/// live; Linux/Windows follow vendor-documented invocations.
/// `os` takes `std::env::consts::OS` values; unknown systems get the
/// generic recipe rather than a macOS-only one that cannot run.
fn external_capture_recipe(os: &str) -> &'static str {
    match os {
        "macos" => {
            "Capture it through the shell instead: run headless Chrome via exec_command (e.g. `'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' --headless --screenshot=/tmp/shot.png --window-size=1280,800 --virtual-time-budget=8000 URL`; never pass `--no-sandbox` — permission review blocks sandbox-disabling flags), or `screencapture -x /tmp/shot.png` for the desktop."
        }
        "linux" => {
            "Capture it through the shell instead: run headless Chrome via exec_command (`$(command -v google-chrome || command -v chromium) --headless --screenshot=/tmp/shot.png --window-size=1280,800 --virtual-time-budget=8000 URL`; never pass `--no-sandbox` — permission review blocks sandbox-disabling flags), or the session screenshot tool for the desktop (`gnome-screenshot`, `grim` on Wayland)."
        }
        "windows" => {
            "Capture it through the shell instead: run headless Chrome via exec_command (`& \"C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe\" --headless --screenshot=$env:TEMP/shot.png --window-size=1280,800 --virtual-time-budget=8000 URL`; never pass `--no-sandbox` — permission review blocks sandbox-disabling flags), a PowerShell System.Drawing snippet for the desktop, or COM automation for Office apps."
        }
        _ => {
            "Capture it through the shell instead: run headless Chrome via exec_command (`chrome --headless --screenshot=<out.png> --window-size=1280,800 --virtual-time-budget=8000 URL`; never pass `--no-sandbox`), or the OS screenshot tool for the desktop."
        }
    }
}

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
        "external address outside the offline preview" => external_capture_recipe(std::env::consts::OS),
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
        ComputerTurnOutcomeKind::ReviewHalted { verdict } => match verdict {
            BatchReviewVerdict::Halt => {
                vec!["Review halted the turn: the batch was judged SUSPECT.".to_string()]
            }
            BatchReviewVerdict::Unreviewed { reason } => {
                vec![format!(
                    "Review halted the turn: the batch could not be screened ({reason}); nothing executed."
                )]
            }
            // Unreachable: a Proceed verdict never halts. Degrade to the
            // generic halt rather than naming a verdict that did not happen.
            BatchReviewVerdict::Proceed => vec!["Review halted the turn.".to_string()],
        },
        ComputerTurnOutcomeKind::Cancelled => vec!["Computer turn cancelled.".to_string()],
        ComputerTurnOutcomeKind::Failed { stage, message } => {
            vec![format!("Computer turn failed at the {stage:?} stage: {message}")]
        }
        ComputerTurnOutcomeKind::Completed { results, summary, preview } => {
            let mut lines = vec![format!(
                "Computer turn completed ({}): {} units.",
                outcome.model, summary.executed_units
            )];
            lines.push(summary_line(summary));
            if let Some(preview) = preview {
                lines.push(format!(
                    "Screenshot captured ({}; ephemeral, not stored).",
                    vtcode_commons::formatting::format_size(preview.bytes)
                ));
            }
            lines.extend(result_report_lines(results));
            lines
        }
    }
}

/// Redacted one-line audit: routing metadata only (target, kinds, teardown,
/// recipient, review state) — never typed text or image bytes. Shared by the
/// model-facing tool result and the durable completion event, so the two can
/// never disagree about what ran.
fn summary_line(summary: &RedactedTurnSummary) -> String {
    let review = match &summary.review {
        None => String::new(),
        Some(BatchReviewVerdict::Proceed) => "; reviewed".to_string(),
        Some(BatchReviewVerdict::Halt) => "; review halted".to_string(),
        Some(BatchReviewVerdict::Unreviewed { reason }) => format!("; review unreviewed ({reason})"),
    };
    let noun = if summary.action_count == 1 { "action" } else { "actions" };
    format!(
        "{} {} ({} {noun}: {}): target {} ({}); screenshots to {}{review}",
        summary.target_kind,
        summary.target,
        summary.action_count,
        summary.action_kinds.join(", "),
        match &summary.teardown {
            TeardownOutcome::NotLaunched => "not launched".to_string(),
            TeardownOutcome::Removed => "removed".to_string(),
            TeardownOutcome::AlreadyGone => "already gone".to_string(),
            TeardownOutcome::Failed(error) => format!("teardown failed: {error}"),
        },
        summary.teardown_trigger,
        summary.recipient,
    )
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
    use super::super::review::BatchReviewVerdict;
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
        // The model cargo-cults `--no-sandbox` onto headless Chrome from
        // container lore; the auto permission review correctly blocks it
        // (rule 5, sandbox-disabling flag) and the model rightly treats
        // the denial as terminal instead of retrying. Prohibit the flag
        // in the recipe itself — verified working without it.
        let external = turn_report_lines(&outcome("external address outside the offline preview"));
        assert!(
            external[0].contains("never pass `--no-sandbox`"),
            "recipe must prohibit the blocked flag: {}",
            external[0]
        );
    }

    #[test]
    fn external_capture_recipe_covers_each_os_without_leakage() {
        // Host-independent: every branch is pinned directly, so Linux and
        // Windows recipes are tested even though this host runs macOS.
        // Chrome flags stay identical; only locations and desktop tools
        // vary. Every branch carries the `--no-sandbox` prohibition.
        for (os, needles, forbidden) in [
            (
                "macos",
                vec![
                    "Google Chrome.app/Contents/MacOS",
                    "screencapture -x",
                    "--virtual-time-budget=8000",
                ],
                vec!["C:/Temp", "grim", "COM automation"],
            ),
            (
                "linux",
                vec![
                    "command -v google-chrome || command -v chromium",
                    "grim",
                    "--virtual-time-budget=8000",
                ],
                vec!["Google Chrome.app", "screencapture", "C:/Temp"],
            ),
            (
                // C:/Temp stays forbidden: stock Windows has no such
                // directory, so the recipe uses $env:TEMP instead.
                "windows",
                vec![
                    "$env:TEMP/shot.png",
                    "Program Files\\Google\\Chrome",
                    "System.Drawing",
                    "COM automation",
                    "--virtual-time-budget=8000",
                ],
                vec!["Google Chrome.app", "screencapture", "grim", "C:/Temp"],
            ),
            (
                "fuchsia",
                vec!["<out.png>", "OS screenshot tool", "--virtual-time-budget=8000"],
                vec!["Google Chrome.app", "screencapture", "grim", "C:/Temp"],
            ),
        ] {
            let recipe = external_capture_recipe(os);
            assert!(recipe.contains("never pass `--no-sandbox`"), "{os}");
            for needle in needles {
                assert!(recipe.contains(needle), "{os} missing {needle}: {recipe}");
            }
            for banned in forbidden {
                assert!(!recipe.contains(banned), "{os} leaks {banned}: {recipe}");
            }
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
            (ComputerTurnOutcomeKind::ReviewHalted { verdict: BatchReviewVerdict::Halt }, "SUSPECT"),
            (
                ComputerTurnOutcomeKind::ReviewHalted {
                    verdict: BatchReviewVerdict::Unreviewed { reason: "timed out" },
                },
                "could not be screened (timed out)",
            ),
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
            preview: None,
        }));
        assert!(lines[0].contains("completed (test-model): 1 units"), "{lines:?}");
        assert!(
            lines.iter().any(|line| line
                == "browser fixture-app (1 action: screenshot): target removed (finished); screenshots to test-model (test)"),
            "{lines:?}"
        );
        assert!(lines.iter().any(|line| line == "call c1 (#1): ok"), "{lines:?}");
        assert!(lines.iter().any(|line| line == "call c2 (#3): error"), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("secret")), "{lines:?}");
        let lines = turn_report_lines(&outcome(ComputerTurnOutcomeKind::Completed {
            results: NativeTurnResults::OpenAi(vec![shot]),
            summary,
            preview: None,
        }));
        assert!(lines.iter().any(|line| line == "call call_1: ok"), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("SECRET")), "{lines:?}");
        assert!(!lines.iter().any(|line| line.contains("Screenshot captured")), "{lines:?}");
    }

    #[test]
    fn completed_report_names_the_retained_screenshot_without_pixels() {
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
        let preview = vtcode_core::exec::events::AppPreviewItem {
            uri: "computer-preview://row-1".to_string(),
            mime_type: Some("image/png".to_string()),
            bytes: 412_000,
            blocks: 1,
            ui: None,
        };
        let outcome = ComputerTurnOutcome {
            kind: ComputerTurnOutcomeKind::Completed {
                results: NativeTurnResults::OpenAi(vec![]),
                summary,
                preview: Some(Box::new(preview)),
            },
            model: "test-model".to_string(),
            usage: None,
        };
        let lines = turn_report_lines(&outcome);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Screenshot captured (") && line.contains("ephemeral, not stored")),
            "{lines:?}"
        );
        assert!(!lines.iter().any(|line| line.contains("computer-preview://")), "{lines:?}");
    }

    #[test]
    fn completed_summary_line_names_routing_and_review_state() {
        // The audit line carries routing metadata only: target, kinds,
        // teardown, recipient, and the review verdict when the review ran.
        let summary = |review| RedactedTurnSummary {
            target_kind: "browser",
            target: "fixture-app".to_string(),
            recipient: "test-model (test)".to_string(),
            action_kinds: vec!["click".to_string()],
            action_count: 1,
            executed_units: 1,
            review,
            teardown_trigger: "finished",
            teardown: TeardownOutcome::Removed,
        };
        for (review, needle) in [
            (None, "; screenshots to test-model (test)"),
            (Some(BatchReviewVerdict::Proceed), "; reviewed"),
            (Some(BatchReviewVerdict::Halt), "; review halted"),
            (Some(BatchReviewVerdict::Unreviewed { reason: "timed out" }), "; review unreviewed (timed out)"),
        ] {
            let line = summary_line(&summary(review));
            assert!(line.contains(needle), "{line:?} missing {needle}");
            assert!(line.contains("browser fixture-app (1 action: click)"), "{line:?}");
            assert!(line.contains("target removed (finished)"), "{line:?}");
        }
        // Asymmetric: a failed teardown flips the target phrase, never the shape.
        let mut failed = summary(None);
        failed.teardown = TeardownOutcome::Failed("boom".to_string());
        assert!(summary_line(&failed).contains("target teardown failed: boom (finished)"));
    }
}
