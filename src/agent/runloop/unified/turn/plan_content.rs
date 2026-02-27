use serde_json::Value;
use vtcode_tui::{PlanContent, PlanPhase, PlanStep};

/// Parses a plan content structure from a JSON value.
pub(crate) fn parse_plan_content_from_json(json: &Value) -> PlanContent {
    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Implementation Plan")
        .to_string();

    let summary = json
        .get("summary")
        .or_else(|| json.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let file_path = json
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let raw_content = json
        .get("raw_content")
        .or_else(|| json.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let open_questions = json
        .get("open_questions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|q| q.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut step_number = 0;
    let phases: Vec<PlanPhase> = json
        .get("phases")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|phase| {
                    let name = phase.get("name").and_then(|v| v.as_str())?.to_string();
                    let steps: Vec<PlanStep> = phase
                        .get("steps")
                        .and_then(|v| v.as_array())
                        .map(|steps_arr| {
                            steps_arr
                                .iter()
                                .filter_map(|step| {
                                    step_number += 1;
                                    let step_desc =
                                        step.get("description").and_then(|v| v.as_str())?;
                                    let details = step
                                        .get("details")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string());
                                    let files = step
                                        .get("files")
                                        .and_then(|v| v.as_array())
                                        .map(|f| {
                                            f.iter()
                                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                                .collect()
                                        })
                                        .unwrap_or_default();
                                    let completed = step
                                        .get("completed")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);

                                    Some(PlanStep {
                                        number: step_number,
                                        description: step_desc.to_string(),
                                        details,
                                        files,
                                        completed,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let completed = steps.iter().all(|step| step.completed) && !steps.is_empty();

                    Some(PlanPhase {
                        name,
                        steps,
                        completed,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let total_steps = phases.iter().map(|p| p.steps.len()).sum();
    let completed_steps = phases
        .iter()
        .map(|phase| phase.steps.iter().filter(|s| s.completed).count())
        .sum();

    PlanContent {
        title,
        summary,
        file_path,
        phases,
        open_questions,
        raw_content,
        total_steps,
        completed_steps,
    }
}
