//! Observed Copilot call state and cumulative-output presentation adapters.

use anstyle::Color;
use serde_json::Value;
use vtcode_core::config::PtyConfig;
use vtcode_core::copilot::{CopilotObservedToolCall, CopilotObservedToolCallStatus};
use vtcode_core::utils::style_helpers::ColorPalette;
use vtcode_ui::tui::app::InlineHandle;

use crate::agent::runloop::unified::progress::ProgressReporter;

use super::presentation::CopilotPtyStream;

pub(super) struct ObservedToolCallState {
    tool_name: String,
    started: bool,
    finished: bool,
    last_output: Option<String>,
    pty_stream: Option<CopilotPtyStream>,
}

impl ObservedToolCallState {
    pub(super) fn new(tool_name: String) -> Self {
        Self {
            tool_name,
            started: false,
            finished: false,
            last_output: None,
            pty_stream: None,
        }
    }

    pub(super) fn tool_name(&self) -> &str {
        &self.tool_name
    }

    pub(super) fn apply(
        &mut self,
        update: &CopilotObservedToolCall,
        tail_limit: usize,
        handle: &InlineHandle,
        pty_config: &PtyConfig,
    ) -> ObservedToolUpdate {
        if self.tool_name == "copilot_tool" && update.tool_name != "copilot_tool" {
            self.tool_name = update.tool_name.clone();
        }

        let started = if !self.started {
            self.started = true;
            true
        } else {
            false
        };

        if started
            && self.pty_stream.is_none()
            && let Some(cmd) = observed_tool_command_display(update)
        {
            self.pty_stream =
                Some(CopilotPtyStream::start(handle, ProgressReporter::new(), tail_limit, cmd, pty_config.clone()));
        }

        let output_snapshot = if let Some(output) = update.output.as_deref().filter(|t| !t.trim().is_empty())
            && self.last_output.as_deref() != Some(output)
        {
            if let Some(delta) = observed_tool_output_delta(self.last_output.as_deref(), output)
                && !delta.is_empty()
                && let Some(stream) = self.pty_stream.as_ref()
            {
                stream.push_output(delta);
            }
            self.last_output = Some(output.to_string());
            Some(output.to_string())
        } else {
            None
        };

        let finished = !self.finished
            && matches!(
                update.status,
                CopilotObservedToolCallStatus::Completed | CopilotObservedToolCallStatus::Failed
            );
        if finished {
            self.finished = true;
            let _ = self
                .pty_stream
                .take()
                .map(|s| s.finish(copilot_observed_status_color(update.status)));
        }

        ObservedToolUpdate { started, output_snapshot, finished }
    }
}

pub(super) struct ObservedToolUpdate {
    pub(super) started: bool,
    pub(super) output_snapshot: Option<String>,
    pub(super) finished: bool,
}

fn extract_command_from_args(arguments: Option<&Value>) -> Option<String> {
    let arguments = arguments?;
    // Display-only extraction shared with tool summaries. The previous
    // per-key loop returned `None` via `?` when the `command` key was absent,
    // never reaching `cmd`/`raw_command`; the canonical helper scans every
    // key and also covers the legacy `bash_command` key.
    vtcode_core::tools::command_args::extract_command_text_with_key(arguments).map(|(text, _)| text)
}

fn copilot_observed_status_color(status: CopilotObservedToolCallStatus) -> Color {
    let palette = ColorPalette::default();
    match status {
        CopilotObservedToolCallStatus::Completed => palette.success,
        CopilotObservedToolCallStatus::Failed => palette.error,
        CopilotObservedToolCallStatus::Pending | CopilotObservedToolCallStatus::InProgress => palette.warning,
    }
}

fn observed_tool_command_display(update: &CopilotObservedToolCall) -> Option<String> {
    extract_command_from_args(update.arguments.as_ref()).or_else(|| {
        update
            .tool_name
            .strip_prefix("Run ")
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToString::to_string)
    })
}

fn observed_tool_output_delta<'a>(previous: Option<&str>, current: &'a str) -> Option<&'a str> {
    if current.is_empty() {
        return None;
    }

    match previous {
        None => Some(current),
        Some(prev) if prev == current => None,
        Some(prev) if current.starts_with(prev) => Some(&current[prev.len()..]),
        Some(prev) => {
            let prefix_len = calculate_common_prefix_len(prev, current);
            if prefix_len == 0 || prefix_len >= current.len() {
                Some(current)
            } else {
                Some(&current[prefix_len..])
            }
        }
    }
}

fn calculate_common_prefix_len(left: &str, right: &str) -> usize {
    let mut bytes = 0;
    for (left_char, right_char) in left.chars().zip(right.chars()) {
        if left_char != right_char {
            break;
        }
        bytes += left_char.len_utf8();
    }
    bytes
}

#[cfg(test)]
mod tests;
