use super::super::output_sections::human_output_option;
use super::*;
use vtcode_exec_events::{ThreadItem, ThreadItemDetails, ToolOutputItem};

impl DesktopView {
    /// Resolve a live tool output to its invocation row when identities are unambiguous.
    ///
    /// Merges into any tool row the `call_id` names — shell, MCP, read, edit,
    /// or search — so one tool call renders as exactly one row. A row that
    /// already absorbed a different capture keeps it. A poll run shows the
    /// latest call's capture, while late captures from earlier calls retain
    /// their own row.
    pub(super) fn runtime_output_row_id(&self, item: &ThreadItem) -> String {
        let ThreadItemDetails::ToolOutput(output) = &item.details else {
            return item.id.clone();
        };
        if self.transcript.position_by_id(&item.id).is_some() {
            return item.id.clone();
        }
        let call_id = self.transcript.resolve_coalesced_call(&output.call_id);
        let Some(row) = self
            .transcript
            .position_by_id(call_id)
            .and_then(|index| self.transcript.get(index))
        else {
            return item.id.clone();
        };
        if row.is_tool() && row.accepts_tool_capture(&item.id, &output.call_id) {
            call_id.to_owned()
        } else {
            item.id.clone()
        }
    }

    pub(super) fn mark_runtime_output_truncated(&mut self, row_id: &str, capture_id: &str, truncated: bool) {
        let keeps_previous = self
            .transcript
            .position_by_id(row_id)
            .and_then(|index| self.transcript.get(index))
            .is_some_and(|row| keep_previous_poll_capture(row, capture_id, true));
        if !keeps_previous {
            self.transcript.set_output_truncated(row_id, truncated);
        }
    }

    pub(super) fn accepts_runtime_item_turn(&self, item: &ThreadItem, row_id: &str, turn_id: Option<&str>) -> bool {
        if !self.transcript.accepts_item_turn(&item.id, turn_id) || !self.transcript.accepts_item_turn(row_id, turn_id)
        {
            return false;
        }
        let call_id = match &item.details {
            ThreadItemDetails::ToolOutput(output) => &output.call_id,
            _ => &item.id,
        };
        let survivor = self.transcript.resolve_coalesced_call(call_id);
        let Some(row) = self
            .transcript
            .position_by_id(survivor)
            .and_then(|index| self.transcript.get(index))
            .filter(|row| row.session_poll.is_some())
        else {
            return true;
        };
        let canonical_turn = item
            .context
            .as_deref()
            .map(|context| context.turn_id.as_str())
            .filter(|id| !id.is_empty());
        self.transcript.accepts_item_turn(survivor, turn_id)
            && !matches!((row.canonical_turn_id.as_deref(), canonical_turn), (Some(a), Some(b)) if a != b)
    }

    pub(super) fn merge_runtime_tool_output(
        &mut self,
        item: &ThreadItem,
        output: &ToolOutputItem,
        historical: bool,
    ) -> bool {
        if historical {
            let call_id = self
                .restored_poll_redirects
                .get(&output.call_id)
                .map(String::as_str)
                .unwrap_or(output.call_id.as_str());
            let incoming_turn = item
                .context
                .as_deref()
                .map(|context| context.turn_id.as_str())
                .filter(|id| !id.is_empty());
            let index = self.restored_activity.iter().position(|row| {
                row.id == call_id
                    && row.is_tool()
                    && row.accepts_tool_capture(&item.id, &output.call_id)
                    && !matches!((row.canonical_turn_id.as_deref(), incoming_turn), (Some(a), Some(b)) if a != b)
            });
            if let Some(index) = index {
                let (_, mut status) = describe_activity(&item.details, None);
                if ActivityState::from_status(&status) == ActivityState::Running {
                    status = tool_status::INTERRUPTED.to_string();
                }
                let Some(row) = self.restored_activity.get_mut(index) else {
                    return true;
                };
                let payload = activity_payload(&row.text);
                row.text = activity_text(&status, payload).into();
                // Identity implies output: `output_for` resolves a selection by
                // identity and falls back to the newest capture when the row is
                // output-less, so a blank merge must not plant an identity.
                let human = human_output_option(&output.output);
                if !keep_previous_poll_capture(row, &item.id, human.is_none()) {
                    row.output = human.map(Into::into);
                    row.output_item_id = row.output.as_ref().map(|_| item.id.clone());
                    row.app_previews.clone_from(&output.app_previews);
                }
                self.bump_restored_activity_revision();
                return true;
            }
        }
        if !historical {
            let row_id = self.runtime_output_row_id(item);
            if row_id != item.id
                && let Some(row) = self
                    .transcript
                    .position_by_id(&row_id)
                    .and_then(|index| self.transcript.get(index))
            {
                let row_is_poll = row.session_poll.is_some();
                let human = human_output_option(&output.output);
                let keep_previous = keep_previous_poll_capture(row, &item.id, human.is_none());
                let title = row.title.to_string();
                let payload = activity_payload(&row.text);
                let status = describe_activity(&item.details, None).1;
                let text = activity_text(&status, payload);
                self.append_or_update_item(row_id.clone(), TranscriptRole::Tool.as_str(), &title, &text, true);
                // Identity implies output (see the historical merge above): a
                // blank capture stores no output and plants no identity.
                let replace_previews = row_is_poll || !output.app_previews.is_empty();
                if keep_previous {
                    return true;
                }
                self.transcript
                    .set_output_identity(&row_id, human.as_ref().map(|_| item.id.as_str()));
                self.transcript.set_output(&row_id, human.map(Into::into));
                if replace_previews {
                    self.transcript.set_app_previews(&row_id, output.app_previews.clone());
                }
                return true;
            }
        }
        false
    }
}

fn keep_previous_poll_capture(row: &TranscriptItem, capture_id: &str, blank: bool) -> bool {
    blank && row.session_poll.is_some() && row.output_item_id.as_deref().is_some_and(|previous| previous != capture_id)
}
