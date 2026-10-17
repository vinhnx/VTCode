use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::ui_interaction::{PlaceholderSpinner, start_loading_status};
use vtcode_commons::ui_protocol::{ProgressPhase, ProgressUpdate};
use vtcode_ui::tui::app::InlineHandle;

enum ProbeSpinner<'a> {
    Owned(PlaceholderSpinner),
    Borrowed(&'a PlaceholderSpinner),
}

/// A parallel result borrows the batch owner rather than starting another writer.
pub(super) struct ProbeStatus<'a> {
    spinner: ProbeSpinner<'a>,
    previous_message: Option<String>,
    previous_phase: Option<ProgressUpdate>,
    stop: &'a CtrlCState,
    handle: &'a InlineHandle,
}

pub(super) fn stopped(stop: &CtrlCState) -> bool {
    stop.is_cancel_requested() || stop.is_cancel_handled() || stop.is_exit_requested()
}

impl<'a> ProbeStatus<'a> {
    pub(super) fn new(
        handle: &'a InlineHandle,
        input_status: &InputStatusState,
        stop: &'a CtrlCState,
        batch_spinner: Option<&'a PlaceholderSpinner>,
    ) -> Self {
        let (spinner, previous_message) = match batch_spinner {
            Some(spinner) => (ProbeSpinner::Borrowed(spinner), spinner.replace_message("Checking tool output...")),
            None => (ProbeSpinner::Owned(start_loading_status(handle, input_status, "Checking tool output...")), None),
        };
        let previous_phase = handle.replace_progress_phase(ProgressPhase::CheckingToolOutput);
        Self {
            spinner,
            previous_message,
            previous_phase,
            stop,
            handle,
        }
    }
}

impl Drop for ProbeStatus<'_> {
    fn drop(&mut self) {
        let spinner = match &self.spinner {
            ProbeSpinner::Owned(spinner) => spinner,
            ProbeSpinner::Borrowed(spinner) => spinner,
        };
        if let Some(ProgressUpdate::Phase { operation, phase }) = self.previous_phase
            && self.handle.current_progress_operation() == Some(operation)
        {
            if stopped(self.stop) {
                self.handle.update_progress(ProgressUpdate::Finish { operation });
            } else {
                self.handle.set_progress_phase(phase);
            }
        }
        if stopped(self.stop) {
            spinner.finish_with_restore(false);
            self.handle.set_input_status(None, None);
        } else if let Some(previous) = self.previous_message.take() {
            spinner.restore_message(previous);
        }
    }
}

#[cfg(test)]
mod tests;
