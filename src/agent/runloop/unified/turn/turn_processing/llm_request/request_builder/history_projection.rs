//! Request-only retirement of expired control messages and duplicate tracker observations.

use std::collections::HashSet;
use vtcode_core::llm::provider as uni;

pub(super) fn project_runtime_history(
    messages: &mut Vec<uni::Message>,
    planning_active: bool,
    approved_execution: bool,
) {
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
