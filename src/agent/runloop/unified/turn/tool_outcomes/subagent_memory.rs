use anyhow::Result;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::persistent_memory::GroundedFactRecord;

use crate::agent::runloop::unified::turn::compaction::{
    SessionMemoryEnvelopeUpdate, refresh_session_memory_envelope,
};
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ParsedSubagentSummary {
    pub(super) summary: Vec<String>,
    pub(super) facts: Vec<String>,
    pub(super) touched_files: Vec<String>,
    pub(super) verification: Vec<String>,
    pub(super) open_questions: Vec<String>,
}

pub(super) fn request_user_input_result_stats(output: &serde_json::Value) -> (usize, bool) {
    let cancelled = output
        .get("cancelled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if cancelled {
        return (0, true);
    }

    let Some(answers) = output.get("answers").and_then(serde_json::Value::as_object) else {
        return (0, true);
    };

    let answered_questions = answers
        .values()
        .filter(|answer| {
            let selected_count = answer
                .get("selected")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            let has_other = answer
                .get("other")
                .and_then(serde_json::Value::as_str)
                .map(|text| !text.trim().is_empty())
                .unwrap_or(false);
            selected_count > 0 || has_other
        })
        .count();
    let cancelled = answered_questions == 0;
    (answered_questions, cancelled)
}

pub(super) fn record_request_user_input_interview_result(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    output: Option<&serde_json::Value>,
) {
    if tool_name != tool_names::REQUEST_USER_INPUT {
        return;
    }

    let (answered_questions, cancelled) = output
        .map(request_user_input_result_stats)
        .unwrap_or((0, true));
    ctx.session_stats
        .record_plan_mode_interview_result(answered_questions, cancelled);
}

fn normalize_subagent_section_items(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let item = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .unwrap_or(line)
                .trim();
            (!item.eq_ignore_ascii_case("none")).then_some(item.to_string())
        })
        .collect()
}

pub(super) fn parse_subagent_summary_markdown(summary: &str) -> Option<ParsedSubagentSummary> {
    #[derive(Clone, Copy)]
    enum Section {
        Summary,
        Facts,
        TouchedFiles,
        Verification,
        OpenQuestions,
    }

    let mut current = None;
    let mut summary_lines = Vec::new();
    let mut fact_lines = Vec::new();
    let mut touched_files = Vec::new();
    let mut verification = Vec::new();
    let mut open_questions = Vec::new();
    let mut saw_contract = false;

    for raw_line in summary.replace("\r\n", "\n").lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        current = match line {
            "## Summary" => {
                saw_contract = true;
                Some(Section::Summary)
            }
            "## Facts" => {
                saw_contract = true;
                Some(Section::Facts)
            }
            "## Touched Files" => {
                saw_contract = true;
                Some(Section::TouchedFiles)
            }
            "## Verification" => {
                saw_contract = true;
                Some(Section::Verification)
            }
            "## Open Questions" => {
                saw_contract = true;
                Some(Section::OpenQuestions)
            }
            _ => current,
        };

        if line.starts_with("## ") {
            continue;
        }

        match current {
            Some(Section::Summary) => summary_lines.push(line.to_string()),
            Some(Section::Facts) => fact_lines.push(line.to_string()),
            Some(Section::TouchedFiles) => touched_files.push(line.to_string()),
            Some(Section::Verification) => verification.push(line.to_string()),
            Some(Section::OpenQuestions) => open_questions.push(line.to_string()),
            None => {}
        }
    }

    saw_contract.then(|| ParsedSubagentSummary {
        summary: normalize_subagent_section_items(&summary_lines),
        facts: normalize_subagent_section_items(&fact_lines),
        touched_files: normalize_subagent_section_items(&touched_files),
        verification: normalize_subagent_section_items(&verification),
        open_questions: normalize_subagent_section_items(&open_questions),
    })
}

fn extract_completed_subagent_entries(output: &serde_json::Value) -> Vec<&serde_json::Value> {
    let mut entries = Vec::new();

    if output.get("completed").and_then(serde_json::Value::as_bool) == Some(true)
        && let Some(entry) = output.get("entry")
    {
        entries.push(entry);
    }

    if output
        .get("status")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|status| status == "completed")
    {
        entries.push(output);
    }

    entries
}

pub(super) fn build_subagent_memory_update(
    output: &serde_json::Value,
) -> Option<SessionMemoryEnvelopeUpdate> {
    let entries = extract_completed_subagent_entries(output);
    if entries.is_empty() {
        return None;
    }

    let mut update = SessionMemoryEnvelopeUpdate::default();
    let mut saw_summary = false;
    for entry in entries {
        let Some(summary) = entry.get("summary").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if summary.trim().is_empty() {
            continue;
        }

        let agent_name = entry
            .get("agent_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("subagent");
        saw_summary = true;

        if let Some(parsed) = parse_subagent_summary_markdown(summary) {
            update
                .grounded_facts
                .extend(parsed.facts.into_iter().map(|fact| GroundedFactRecord {
                    fact,
                    source: format!("subagent:{agent_name}"),
                }));
            update.touched_files.extend(parsed.touched_files);
            update.open_questions.extend(parsed.open_questions);
            update.verification_todo.extend(parsed.verification);
            if !parsed.summary.is_empty() {
                update
                    .delegation_notes
                    .push(format!("{agent_name}: {}", parsed.summary.join(" | ")));
            }
        } else {
            update
                .delegation_notes
                .push(format!("{agent_name}: {}", summary.trim()));
        }
    }

    (saw_summary
        && (!update.grounded_facts.is_empty()
            || !update.touched_files.is_empty()
            || !update.open_questions.is_empty()
            || !update.verification_todo.is_empty()
            || !update.delegation_notes.is_empty()))
    .then_some(update)
}

pub(super) fn merge_subagent_completion_into_memory(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    output: &serde_json::Value,
) -> Result<()> {
    if !matches!(
        tool_name,
        tool_names::SPAWN_AGENT
            | tool_names::SEND_INPUT
            | tool_names::WAIT_AGENT
            | tool_names::RESUME_AGENT
            | tool_names::CLOSE_AGENT
    ) {
        return Ok(());
    }

    let session_id = ctx.tool_registry.harness_context_snapshot().session_id;
    let Some(update) = build_subagent_memory_update(output) else {
        return Ok(());
    };

    if !update.touched_files.is_empty() {
        ctx.session_stats
            .record_touched_files(update.touched_files.iter().cloned());
    }

    refresh_session_memory_envelope(
        ctx.config.workspace.as_path(),
        &session_id,
        ctx.vt_cfg,
        ctx.working_history,
        ctx.session_stats,
        Some(&update),
    )?;

    Ok(())
}
