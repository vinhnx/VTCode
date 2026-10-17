//! Inline PTY presentation shared by observed calls and local terminal monitors.

use anstyle::Color;
use vtcode_core::config::PtyConfig;
use vtcode_core::tools::registry::ToolProgressCallback;
use vtcode_ui::tui::app::InlineHandle;

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::tool_pipeline::PtyStreamRuntime;
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;

pub(super) struct CopilotPtyStream {
    progress_reporter: ProgressReporter,
    spinner: PlaceholderSpinner,
    runtime: PtyStreamRuntime,
    callback: ToolProgressCallback,
}

impl CopilotPtyStream {
    pub(super) fn start(
        handle: &InlineHandle,
        progress_reporter: ProgressReporter,
        tail_limit: usize,
        command_display: String,
        pty_config: PtyConfig,
    ) -> Self {
        let spinner = PlaceholderSpinner::with_progress(
            handle,
            None,
            None,
            format!("Running command: {command_display}"),
            Some(&progress_reporter),
        );
        spinner.set_defer_restore(true);
        let (runtime, callback) = PtyStreamRuntime::start(
            handle.clone(),
            progress_reporter.clone(),
            tail_limit,
            Some(command_display),
            pty_config,
            None,
            true,
        );

        Self { progress_reporter, spinner, runtime, callback }
    }

    pub(super) fn push_output(&self, chunk: &str) {
        (self.callback)("exec_command", chunk);
    }

    pub(super) fn finish(self, header_color: Color) {
        self.spinner.finish();
        let progress_reporter = self.progress_reporter.clone();
        let runtime = self.runtime;
        drop(self.callback);

        tokio::spawn(async move {
            progress_reporter.complete().await;
            runtime.shutdown(header_color).await;
        });
    }
}

#[cfg(test)]
mod tests;
