//! Request-only retirement of expired controls and redundant runtime observations.

use std::collections::HashSet;
use vtcode_core::llm::provider as uni;
use vtcode_core::types::CompactStr;

use crate::agent::runloop::unified::turn::tool_outcomes::helpers::HARNESS_AUTO_VERIFY_CALL_ID;

const VERIFIER_WAIT_OUTPUT_BYTES: usize = 16_384;

struct VerifierBatch {
    start: usize,
    end: usize,
    response_index: usize,
    session_id: CompactStr,
    is_wait: bool,
    terminal: bool,
    output: String,
}

/// Only runtime-owned, complete call/result batches may be compacted. Unknown
/// result fields (including diagnostics and spool evidence) form boundaries.
fn verifier_batch(messages: &[uni::Message], start: usize) -> Option<VerifierBatch> {
    let first = messages.get(start)?;
    let assistant_index = if first.role == uni::MessageRole::System
        && first.content.as_text().starts_with("Harness auto-verification:")
    {
        start.checked_add(1)?
    } else {
        start
    };
    let assistant = messages.get(assistant_index)?;
    if assistant.role != uni::MessageRole::Assistant
        || !assistant.content.trim().is_empty()
        || assistant.reasoning.is_some()
        || assistant.reasoning_details.is_some()
    {
        return None;
    }
    let [call] = assistant.tool_calls.as_deref()? else {
        return None;
    };
    if call.id != HARNESS_AUTO_VERIFY_CALL_ID {
        return None;
    }
    let function = call.function.as_ref()?;
    let args: serde_json::Value = serde_json::from_str(&function.arguments).ok()?;
    let is_wait = match function.name.as_str() {
        "exec_command" => false,
        "write_stdin" if args["action"] == "wait" => true,
        _ => return None,
    };
    let response_index = assistant_index.checked_add(1)?;
    let response = messages.get(response_index)?;
    if response.role != uni::MessageRole::Tool || response.tool_call_id.as_deref() != Some(HARNESS_AUTO_VERIFY_CALL_ID)
    {
        return None;
    }
    let result: serde_json::Value = serde_json::from_str(&response.content.as_text()).ok()?;
    let fields = result.as_object()?;
    if !fields.keys().all(|key| {
        matches!(
            key.as_str(),
            "output"
                | "backend"
                | "background"
                | "lifecycle_state"
                | "child_pid"
                | "pid"
                | "output_truncated"
                | "total_output_bytes"
                | "next_continue_args"
                | "next_wait_args"
                | "next_action_hint"
                | "waited_seconds"
                | "wait_deadline_seconds"
                | "exit_code"
                | "session_id"
        )
    }) || result["output_truncated"] == true
    {
        return None;
    }
    let returned_session = vtcode_core::tools::command_args::session_id_text(&result)
        .or_else(|| {
            result
                .get("next_wait_args")
                .and_then(vtcode_core::tools::command_args::session_id_text)
        })
        .or_else(|| {
            result
                .get("next_continue_args")
                .and_then(vtcode_core::tools::command_args::session_id_text)
        });
    let session_id = if is_wait {
        let expected = vtcode_core::tools::command_args::session_id_text(&args)?;
        if returned_session.is_some_and(|returned| returned != expected) {
            return None;
        }
        expected
    } else {
        returned_session?
    };
    let terminal = result.get("exit_code").and_then(serde_json::Value::as_i64).is_some();
    if !terminal && result["lifecycle_state"] != "running" {
        return None;
    }
    Some(VerifierBatch {
        start,
        end: response_index.checked_add(1)?,
        response_index,
        session_id: session_id.into(),
        is_wait,
        terminal,
        output: result.get("output").map_or(Some(""), serde_json::Value::as_str)?.to_owned(),
    })
}

fn compact_verifier_waits(messages: &mut Vec<uni::Message>) {
    let mut keep = vec![true; messages.len()];
    let mut index = 0;
    while index < messages.len() {
        let Some(first) = verifier_batch(messages, index) else {
            index += 1;
            continue;
        };
        index = first.end;
        let mut batches = vec![first];
        while let Some(next) = verifier_batch(messages, index) {
            let Some(previous) = batches.last() else { break };
            if previous.terminal || !next.is_wait || previous.session_id != next.session_id {
                break;
            }
            index = next.end;
            batches.push(next);
        }
        if batches.len() < 3 {
            continue;
        }
        let Some(last) = batches.last() else { continue };
        let mut output = String::new();
        let mut output_truncated = false;
        for batch in batches.iter().skip(1).take(batches.len() - 2) {
            if !batch.output.is_empty() {
                output.push_str(&batch.output);
                if output.len() > VERIFIER_WAIT_OUTPUT_BYTES {
                    output_truncated = true;
                    let mut boundary = output.len() - VERIFIER_WAIT_OUTPUT_BYTES;
                    while !output.is_char_boundary(boundary) {
                        boundary += 1;
                    }
                    output.drain(..boundary);
                }
            }
            if let Some(range) = keep.get_mut(batch.start..batch.end) {
                range.fill(false);
            }
        }
        if let Some(response) = messages.get_mut(last.response_index)
            && let Ok(mut result) = serde_json::from_str::<serde_json::Value>(&response.content.as_text())
        {
            result["harness_waits_omitted"] = (batches.len() - 2).into();
            if !output.is_empty() {
                result["prior_harness_wait_output"] = output.into();
                result["prior_harness_wait_output_truncated"] = output_truncated.into();
            }
            response.content = uni::MessageContent::Text(result.to_string());
        }
    }
    let mut flags = keep.into_iter();
    messages.retain(|_| flags.next().unwrap_or(true));
}

pub(super) fn project_runtime_history(
    messages: &mut Vec<uni::Message>,
    planning_active: bool,
    approved_execution: bool,
) {
    compact_verifier_waits(messages);
    let last_user = messages.iter().rposition(|message| message.role == uni::MessageRole::User);
    let last_handoff = messages.iter().rposition(|message| {
        message.role == uni::MessageRole::System
            && message.content.as_text().starts_with("Execution handoff is active.")
    });
    let mut index = 0;
    messages.retain(|message| {
        let current = index;
        index += 1;
        if message.role != uni::MessageRole::System {
            return true;
        }
        let text = message.content.as_text();
        if text.starts_with("Execution handoff is active.") {
            return approved_execution && !planning_active && Some(current) == last_handoff;
        }
        let expired = last_user.is_some_and(|user| current < user)
            || (!planning_active && text.starts_with("Planning recovery: the proposed plan was rejected."));
        !expired || !crate::agent::runloop::unified::turn::turn_loop_helpers::is_turn_scoped_tool_restriction(&text)
    });

    let list_calls: HashSet<_> = messages
        .iter()
        .flat_map(|message| message.tool_calls.iter().flatten())
        .filter(|call| {
            call.function.as_ref().is_some_and(|function| {
                function.name == vtcode_core::config::constants::tools::TASK_TRACKER
                    && serde_json::from_str::<serde_json::Value>(&function.arguments)
                        .is_ok_and(|args| args["action"] == "list")
            })
        })
        .map(|call| call.id.clone())
        .collect();
    let mut seen = HashSet::new();
    for message in messages.iter_mut().rev() {
        if message.role != uni::MessageRole::Tool
            || !message.tool_call_id.as_ref().is_some_and(|id| list_calls.contains(id))
        {
            continue;
        }
        let Ok(output) = serde_json::from_str::<serde_json::Value>(&message.content.as_text()) else {
            continue;
        };
        if output["status"] != "ok" {
            continue;
        }
        let Some(checklist) = output.get("checklist") else {
            continue;
        };
        if !seen.insert(checklist.to_string()) {
            message.content = uni::MessageContent::Text(serde_json::json!({
                "status":"unchanged",
                "message":"Repeated tracker observation omitted. Use the latest full tracker result; update the next task instead of listing again."
            }).to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn verifier_messages(session_id: &str, launch: bool, output: &str, exit: Option<i64>) -> Vec<uni::Message> {
        let name = if launch { "exec_command" } else { "write_stdin" };
        let args = if launch {
            json!({"cmd":"cargo check --locked"})
        } else {
            json!({"action":"wait", "session_id":session_id})
        };
        let mut result = json!({"output":output, "lifecycle_state":if exit.is_some() {"exited"}else{"running"}});
        if let Some(exit) = exit {
            result["exit_code"] = exit.into();
        } else {
            result["next_wait_args"] = json!({"session_id":session_id,"action":"wait","wait_timeout_seconds":600});
            result["next_action_hint"] = "Use the exact returned wait arguments. ".repeat(30).into();
        }
        let mut messages = Vec::new();
        if launch {
            messages.push(uni::Message::system("Harness auto-verification: running the project verifier".into()));
        }
        messages.push(uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                HARNESS_AUTO_VERIFY_CALL_ID.into(),
                name.into(),
                args.to_string(),
            )],
        ));
        messages.push(uni::Message::tool_response(HARNESS_AUTO_VERIFY_CALL_ID.into(), result.to_string()));
        messages
    }

    #[test]
    fn verifier_wait_projection_retains_launch_latest_evidence_and_pairing_without_changing_history() {
        let mut history = verifier_messages("owned-a", true, "launch evidence", None);
        for tick in 0..120 {
            history.extend(verifier_messages(
                "owned-a",
                false,
                if tick == 37 { "distinct middle evidence" } else { "" },
                None,
            ));
        }
        history.extend(verifier_messages("owned-a", false, "terminal evidence", Some(0)));
        let original = history.clone();
        let mut projected = history.clone();
        project_runtime_history(&mut projected, false, false);
        assert_eq!(projected.len(), 5);
        assert!(projected[2].content.as_text().contains("launch evidence"));
        let result: serde_json::Value = serde_json::from_str(&projected[4].content.as_text()).unwrap();
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["output"], "terminal evidence");
        assert_eq!(result["harness_waits_omitted"], 120);
        assert!(
            result["prior_harness_wait_output"]
                .as_str()
                .unwrap()
                .contains("distinct middle evidence")
        );
        assert!(!vtcode_core::core::agent::state::request_history_needs_normalization(&projected));
        assert_eq!(history, original);
        let once = projected.clone();
        project_runtime_history(&mut projected, false, false);
        assert_eq!(projected, once, "projection must be idempotent");
        assert!(serde_json::to_vec(&projected).unwrap().len() * 20 < serde_json::to_vec(&history).unwrap().len());
    }

    #[test]
    fn verifier_wait_projection_preserves_exact_chunks_split_inside_tokens_and_lines() {
        let mut history = verifier_messages("owned-a", true, "launch prefix", None);
        for chunk in ["error: missing sym", "bol ", "", "foo\nline ", "two"] {
            history.extend(verifier_messages("owned-a", false, chunk, None));
        }
        history.extend(verifier_messages("owned-a", false, "\nterminal suffix", Some(1)));
        let original = history.clone();
        project_runtime_history(&mut history, false, false);
        let result: serde_json::Value = serde_json::from_str(&history.last().unwrap().content.as_text()).unwrap();
        assert_eq!(result["prior_harness_wait_output"], "error: missing symbol foo\nline two");
        assert_eq!(result["prior_harness_wait_output_truncated"], false);
        assert_eq!(result["output"], "\nterminal suffix");
        assert_eq!(result["exit_code"], 1);
        assert_eq!(result["harness_waits_omitted"], 5);
        assert!(original[4].content.as_text().contains("error: missing sym"));
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn verifier_wait_projection_bounds_unicode_output_and_preserves_nonzero_terminal_status() {
        let mut history = verifier_messages("owned-a", true, "launch", None);
        for _ in 0..20 {
            history.extend(verifier_messages("owned-a", false, &"界".repeat(3000), None));
        }
        history.extend(verifier_messages("owned-a", false, "last diagnostic line", None));
        history.extend(verifier_messages("owned-a", false, "checker failed", Some(7)));
        project_runtime_history(&mut history, false, false);
        let result: serde_json::Value = serde_json::from_str(&history.last().unwrap().content.as_text()).unwrap();
        assert_eq!(result["exit_code"], 7);
        assert_eq!(result["output"], "checker failed");
        let retained = result["prior_harness_wait_output"].as_str().unwrap();
        assert!(retained.len() <= VERIFIER_WAIT_OUTPUT_BYTES);
        assert!(retained.ends_with("last diagnostic line"));
        assert!(retained.contains('界'));
        assert_eq!(result["prior_harness_wait_output_truncated"], true);
    }

    #[test]
    fn verifier_wait_projection_preserves_control_identity_and_rich_evidence_boundaries() {
        let mut boundaries = vec![
            vec![uni::Message::user("New scope".into())],
            vec![uni::Message::system(
                "Verification is still pending: new mutation generation".into(),
            )],
            verifier_messages("different-session", false, "other session", None),
            verifier_messages("owned-a", true, "new launch", None),
        ];
        for field in [
            "diagnosis",
            "spool_path",
            "reused_recent_result",
            "error",
            "cancelled",
            "unknown_evidence",
            "output_truncated",
        ] {
            let mut batch = verifier_messages("owned-a", false, "retain this evidence", None);
            let response = batch.last_mut().unwrap();
            let mut result: serde_json::Value = serde_json::from_str(&response.content.as_text()).unwrap();
            result[field] = if field == "output_truncated" {
                json!(true)
            } else {
                json!("preserve")
            };
            response.content = uni::MessageContent::Text(result.to_string());
            boundaries.push(batch);
        }
        let mut ordinary = verifier_messages("owned-a", false, "model-owned wait", None);
        ordinary[0].tool_calls.as_mut().unwrap()[0].id = "model-call".into();
        ordinary[1].tool_call_id = Some("model-call".into());
        boundaries.push(ordinary);
        for boundary in boundaries {
            let mut history = verifier_messages("owned-a", true, "launch", None);
            history.extend(verifier_messages("owned-a", false, "before", None));
            history.extend(boundary);
            history.extend(verifier_messages("owned-a", false, "terminal", Some(0)));
            let original = history.clone();
            project_runtime_history(&mut history, false, false);
            assert_eq!(history, original);
        }
    }

    #[test]
    fn expired_controls_and_duplicate_handoffs_are_request_only() {
        let history = vec![
            uni::Message::system("Planning recovery: the proposed plan was rejected. Emit only a plan.".into()),
            uni::Message::system("Execution handoff is active. old approved plan".into()),
            uni::Message::system("Tool follow-up failed. Tools disabled; respond with text.".into()),
            uni::Message::user("Implement the approved plan now.".into()),
            uni::Message::system("Execution handoff is active. current approved plan".into()),
            uni::Message::system("Keep sandbox and permission checks.".into()),
        ];
        let mut projected = history.clone();
        project_runtime_history(&mut projected, false, true);
        assert_eq!(projected.len(), 3);
        assert_eq!(projected[1].content.as_text(), "Execution handoff is active. current approved plan");
        assert_eq!(history.len(), 6);
        assert!(history[0].content.as_text().contains("proposed plan was rejected"));
    }

    #[test]
    fn active_restrictions_and_user_text_are_preserved() {
        let history = vec![
            uni::Message::user("Planning recovery: the proposed plan was rejected.".into()),
            uni::Message::system("Planning recovery: the proposed plan was rejected. Repair it.".into()),
            uni::Message::system("Tool follow-up failed. Tools disabled; respond with text.".into()),
        ];
        let mut projected = history.clone();
        project_runtime_history(&mut projected, true, false);
        assert_eq!(projected, history);
    }

    #[test]
    fn duplicate_observations_keep_latest_full_state_and_all_call_pairs() {
        let mut history = vec![];
        for (id, completed) in [("first", 0), ("second", 1), ("third", 1)] {
            history.push(uni::Message::assistant_with_tools(
                String::new(),
                vec![uni::ToolCall::function(
                    id.into(),
                    "task_tracker".into(),
                    json!({"action":"list"}).to_string(),
                )],
            ));
            history.push(uni::Message::tool_response(
                id.into(),
                json!({"status":"ok", "checklist":{
                    "completed":completed, "items":[{"index":1,"status":if completed==1 {"completed"}else{"pending"}}]
                }})
                .to_string(),
            ));
        }
        let mut projected = history.clone();
        project_runtime_history(&mut projected, false, false);
        assert_eq!(projected.len(), 6);
        assert!(projected[1].content.as_text().contains("pending"));
        assert!(projected[3].content.as_text().contains("Repeated tracker observation omitted"));
        assert!(projected[5].content.as_text().contains("completed"));
        assert!(!history[3].content.as_text().contains("omitted"));
        for (old, new) in history.iter().zip(&projected) {
            assert_eq!(old.tool_call_id, new.tool_call_id);
            assert_eq!(old.tool_calls, new.tool_calls);
        }
    }
}
