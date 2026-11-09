//! Interaction response dispatch: validate a host answer, forward it to the
//! session runtime, and keep the interaction retryable until delivery lands.

use super::*;

pub(super) fn dispatch_interaction_response(
    state: &mut ServerState,
    response: &protocol::InteractionResponse,
    choice_ids: &[String],
    answer_texts: &[Option<String>],
    input: &mpsc::UnboundedSender<InlineEvent>,
) -> Result<()> {
    let pending = state
        .pending_interactions
        .get(&response.interaction_id)
        .cloned()
        .ok_or_else(|| anyhow!("interaction is no longer pending"))?;
    if pending.session_id != response.session_id || pending.turn_id != response.turn_id {
        bail!("interaction response is stale");
    }
    let event = match response.hotkey {
        Some(protocol::InteractionHotkey::LaunchEditor) => {
            host_interaction_hotkey_response(&pending.view, TransientHotkeyAction::LaunchEditor)
                .ok_or_else(|| anyhow!("interaction hotkey is invalid"))?
        }
        None => {
            if choice_ids.iter().any(|choice_id| {
                !pending
                    .view
                    .steps
                    .iter()
                    .any(|step| step.choices.iter().any(|choice| &choice.id == choice_id))
            }) {
                bail!("interaction choice is invalid");
            }
            if pending.view.steps.len() > 1 && !response.cancelled && choice_ids.len() != pending.view.steps.len() {
                bail!("every interaction step requires a response");
            }
            host_interaction_response(&pending.view, choice_ids, answer_texts, response.cancelled)
                .ok_or_else(|| anyhow!("interaction response is incomplete"))?
        }
    };

    // Keep the interaction retryable unless the runtime accepted the event.
    state.interaction_registry.validate(response)?;
    input
        .send(event)
        .map_err(|_error| anyhow!("selected session runtime has stopped"))?;
    state.interaction_registry.resolve(response)?;
    state.pending_interactions.remove(&response.interaction_id);
    Ok(())
}
