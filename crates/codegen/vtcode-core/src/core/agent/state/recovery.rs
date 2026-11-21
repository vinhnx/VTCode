use super::*;

/// Ensure all tool calls have corresponding outputs in the message list.
pub fn ensure_call_outputs_present(messages: &mut Vec<Message>) {
    let report = validate_history_invariants(messages);

    // Create synthetic outputs for missing calls in reverse order to avoid index shifting
    for missing in report.missing_outputs.iter().rev() {
        let synthetic_message = Message::tool_response(
            missing.call_id.as_str().to_string(),
            "canceled: Tool execution was interrupted. This synthetic output was created \
             during history normalization to maintain conversation invariants."
                .to_string(),
        );

        tracing::warn!("Creating synthetic output for call {} due to missing execution result", missing.call_id);

        // Find the position to insert: right after the corresponding call
        let insert_pos = messages
            .iter()
            .position(|msg| {
                msg.tool_calls
                    .as_ref()
                    .is_some_and(|calls| calls.iter().any(|call| call.id == missing.call_id.as_str()))
            })
            .map(|pos| pos + 1);

        if let Some(pos) = insert_pos {
            messages.insert(pos, synthetic_message);
        } else {
            // If we can't find the call, just append the synthetic output
            messages.push(synthetic_message);
        }
    }
}

/// Remove outputs without corresponding calls (orphaned outputs) from the message list.
pub fn remove_orphan_outputs(messages: &mut Vec<Message>) {
    let report = validate_history_invariants(messages);

    if report.orphan_outputs.is_empty() {
        return;
    }

    let orphan_ids: HashSet<String> = report.orphan_outputs.iter().map(|id| id.as_str().to_string()).collect();

    let initial_len = messages.len();

    // Retain only messages that either:
    // - Don't have a tool_call_id (not a tool response)
    // - Have a tool_call_id that matches an existing call
    messages.retain(|msg| {
        if let Some(tool_call_id) = msg.tool_call_id.as_ref()
            && orphan_ids.contains(tool_call_id)
        {
            tracing::warn!("Removing orphan output for call {}", tool_call_id);
            return false;
        }
        true
    });

    if messages.len() != initial_len {
        tracing::info!("Removed {} orphan outputs", initial_len - messages.len());
    }
}

/// Normalize history to enforce call/output pairing invariants.
pub fn normalize_history(messages: &mut Vec<Message>) {
    ensure_call_outputs_present(messages);
    remove_orphan_outputs(messages);

    // Log if issues were found
    let report = validate_history_invariants(messages);
    if !report.is_valid() {
        tracing::warn!("History validation: {}", report.summary());
    } else {
        tracing::debug!("History normalized successfully");
    }
}

/// Recover from crashed or interrupted session by fixing history invariants.
pub fn recover_history_from_crash(messages: &mut Vec<Message>) {
    let report = validate_history_invariants(messages);

    if !report.missing_outputs.is_empty() {
        tracing::warn!("Found {} missing outputs during recovery", report.missing_outputs.len());
        ensure_call_outputs_present(messages);
    }

    if !report.orphan_outputs.is_empty() {
        tracing::warn!("Found {} orphan outputs during recovery", report.orphan_outputs.len());
        remove_orphan_outputs(messages);
    }

    if report.is_valid() {
        tracing::debug!("History invariants are valid");
    }
}
