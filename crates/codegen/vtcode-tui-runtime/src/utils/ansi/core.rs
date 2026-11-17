//! Renderer construction and display-state accessors.

use super::*;

use crate::ui::tui::{InlineHandle, InlineMessageKind};
use anstream::{AutoStream, ColorChoice};
use std::io;
use vtcode_commons::color_policy::{self, ColorOutputPolicySource};
use vtcode_commons::ui_protocol::ToolOutputId;
use vtcode_core::config::ToolDisplayMode;
use vtcode_core::config::loader::SyntaxHighlightingConfig;
use vtcode_core::utils::ansi_capabilities::AnsiCapabilities;

impl AnsiRenderer {
    /// Create a new renderer for stdout
    pub fn stdout() -> Self {
        let mut capabilities = AnsiCapabilities::detect();
        let policy = color_policy::current_color_output_policy();

        if !policy.enabled {
            capabilities.no_color = true;
            capabilities.force_color = false;
        } else if matches!(
            policy.source,
            ColorOutputPolicySource::CliColorAlways | ColorOutputPolicySource::ConfigOverride
        ) {
            capabilities.no_color = false;
            capabilities.force_color = true;
        }

        let color = capabilities.supports_color();
        let choice = if !color {
            ColorChoice::Never
        } else if matches!(
            policy.source,
            ColorOutputPolicySource::CliColorAlways | ColorOutputPolicySource::ConfigOverride
        ) {
            ColorChoice::Always
        } else {
            ColorChoice::Auto
        };
        Self {
            writer: AutoStream::new(io::stdout(), choice),
            buffer: String::with_capacity(1024),
            color,
            sink: None,
            last_line_was_empty: false,
            highlight_config: SyntaxHighlightingConfig::default(),
            capabilities,
            reasoning_visible: true,
            screen_reader_mode: false,
            show_diagnostics_in_transcript: false,
            show_message_timestamps: true,
            pending_response_footer: false,
            defer_turn_footer: false,
            turn_started_at: None,
            tool_display_mode: ToolDisplayMode::Compact,
            communication_style: vtcode_commons::ui_protocol::CommunicationStyle::Balanced,
            diff_preview_mode: vtcode_commons::ui_protocol::DiffPreviewMode::Inline,
            compact_command_group: None,
            next_compact_group_id: 0,
            pending_tool_output_anchor: None,
            session_expand_anchor: None,
            session_body: false,
            diagnostic_sink: None,
            diagnostic_history: std::collections::VecDeque::new(),
        }
    }

    /// Create a renderer that forwards output to the inline UI session handle
    pub fn with_inline_ui(handle: InlineHandle, highlight_config: SyntaxHighlightingConfig) -> Self {
        let mut renderer = Self::stdout();
        renderer.highlight_config = highlight_config.clone();
        renderer.sink = Some(InlineSink::new(handle, highlight_config));
        renderer.last_line_was_empty = false;
        renderer
    }

    /// Own a presentation-only authentication wait on the interactive terminal.
    pub fn program_status_wait(
        &self,
        kind: vtcode_commons::program_status::InteractionKind,
    ) -> Option<vtcode_ui::tui::core_tui::types::ProgramStatusWaitGuard> {
        self.sink.as_ref().map(|sink| sink.handle.program_status_wait(kind))
    }

    /// Override the syntax highlighting configuration.
    pub fn set_highlight_config(&mut self, config: SyntaxHighlightingConfig) {
        if let Some(sink) = &mut self.sink {
            sink.set_highlight_config(config.clone());
        }
        self.highlight_config = config;
    }

    /// Start buffering transcript lines for one batched UI append.
    ///
    /// Used by transcript reloads (resume, rewind) so the viewport paints the
    /// final state once instead of scrolling through intermediate frames.
    pub fn begin_transcript_batch(&mut self) {
        if let Some(sink) = &mut self.sink {
            sink.begin_transcript_batch();
        }
    }

    /// Flush buffered transcript lines as a single UI command.
    pub fn end_transcript_batch(&mut self) {
        if let Some(sink) = &mut self.sink {
            sink.end_transcript_batch();
        }
    }

    /// Run `render` with transcript batching enabled.
    ///
    /// The batch is flushed even when `render` fails, so buffered lines never
    /// leak into an unrelated later call.
    pub fn transcript_batch<T>(&mut self, render: impl FnOnce(&mut Self) -> anyhow::Result<T>) -> anyhow::Result<T> {
        self.begin_transcript_batch();
        let result = render(self);
        self.end_transcript_batch();
        result
    }

    /// Associate the next summary line with a captured tool output block.
    ///
    /// This edge is UI-only. It is carried directly on the summary command so
    /// repeated commands remain associated with their own captures even when
    /// completions are interleaved.
    pub fn set_next_tool_output_anchor(&mut self, id: ToolOutputId) {
        self.pending_tool_output_anchor = Some(id);
    }

    /// Remember which capture a forthcoming exec-session expand notice should
    /// open. Unlike [`Self::set_next_tool_output_anchor`] this survives body
    /// lines and is consumed only when the notice is emitted.
    pub fn set_session_expand_anchor(&mut self, id: ToolOutputId) {
        self.session_expand_anchor = Some(id);
    }

    /// Take the pending session expand anchor, if any.
    pub fn take_session_expand_anchor(&mut self) -> Option<ToolOutputId> {
        self.session_expand_anchor.take()
    }

    /// Mark the upcoming stream body as an exec-session stdin/stdout capture
    /// (dim + expand), even when the tool name alone would look like a launch.
    pub fn set_session_body(&mut self, session_body: bool) {
        self.session_body = session_body;
    }

    /// Whether the upcoming stream body should use the exec-session path.
    pub fn session_body_active(&self) -> bool {
        self.session_body
    }

    /// Check if the last line rendered was empty
    pub fn was_previous_line_empty(&self) -> bool {
        self.last_line_was_empty
    }

    pub(super) fn message_kind(style: MessageStyle) -> InlineMessageKind {
        style.message_kind()
    }

    pub fn supports_streaming_markdown(&self) -> bool {
        self.sink.is_some()
    }

    /// Determine whether the renderer is connected to the inline UI.
    ///
    /// Inline rendering uses the terminal session scrollback, so tool output should
    /// avoid truncation that would otherwise be applied in compact CLI mode.
    pub fn prefers_untruncated_output(&self) -> bool {
        self.sink.is_some()
    }

    pub fn supports_inline_ui(&self) -> bool {
        self.sink.is_some()
    }

    pub fn set_reasoning_visible(&mut self, visible: bool) {
        self.reasoning_visible = visible;
    }

    pub fn reasoning_visible(&self) -> bool {
        self.reasoning_visible
    }

    /// Whether rendered output is streamed into an inline TUI sink (as opposed to
    /// written directly to a terminal writer).
    pub fn writes_to_inline_sink(&self) -> bool {
        self.sink.is_some()
    }

    pub fn set_screen_reader_mode(&mut self, enabled: bool) {
        self.screen_reader_mode = enabled;
    }

    pub fn set_show_diagnostics_in_transcript(&mut self, enabled: bool) {
        self.show_diagnostics_in_transcript = if cfg!(debug_assertions) { enabled } else { false };
    }

    /// Start a fresh turn: arm footer deferral and drop any pending footer left
    /// by aborted output so it cannot leak into this turn's final message.
    ///
    /// The arm instant also brackets the turn's duration: the footer pairs the
    /// wall clock with the elapsed time between this call and
    /// [`AnsiRenderer::finish_turn_message_timestamp`].
    pub fn begin_turn_message_timestamp(&mut self) {
        self.pending_response_footer = false;
        self.defer_turn_footer = true;
        self.turn_started_at = Some(std::time::Instant::now());
    }

    /// Show one wall-clock footer under the turn's final agent response.
    ///
    /// Enabled by default so headless and unwired paths stay consistent;
    /// sessions apply `ui.show_message_timestamps` through this setter.
    pub fn set_show_message_timestamps(&mut self, enabled: bool) {
        self.show_message_timestamps = enabled;
    }

    pub fn show_message_timestamps(&self) -> bool {
        self.show_message_timestamps
    }

    pub fn set_tool_display_mode(&mut self, mode: ToolDisplayMode) {
        self.flush_compact_command_group();
        self.tool_display_mode = match mode {
            ToolDisplayMode::Compact => ToolDisplayMode::Compact,
            ToolDisplayMode::Expanded | ToolDisplayMode::Unknown => ToolDisplayMode::Expanded,
        };
    }

    pub fn tool_display_mode(&self) -> ToolDisplayMode {
        self.tool_display_mode
    }

    /// How much routine agent-internal failure chatter reaches the transcript.
    /// Sessions apply `ui.communication_style` through this setter; unknown
    /// future values resolve to `Balanced` via [`CommunicationStyle::resolve`](vtcode_commons::ui_protocol::CommunicationStyle::resolve).
    pub fn set_communication_style(&mut self, style: vtcode_commons::ui_protocol::CommunicationStyle) {
        self.communication_style = style.resolve();
    }

    pub fn communication_style(&self) -> vtcode_commons::ui_protocol::CommunicationStyle {
        self.communication_style
    }

    /// Whether a failed result's evidence rows (inline stderr/critical-note
    /// lines and the structured error block) paint beyond the owning tool
    /// row's compact failed marker. Interactive transcripts keep only the
    /// compact marker outside `Verbose`: hidden evidence stays in the
    /// canonical tool-output event, the plain-text trail, and Review.
    /// Headless output is its own surface, so evidence always paints there.
    pub fn failure_evidence_visible(&self) -> bool {
        !self.writes_to_inline_sink()
            || matches!(self.communication_style(), vtcode_commons::ui_protocol::CommunicationStyle::Verbose)
    }

    /// Whether once-per-turn harness coaching notices (verification
    /// re-run prompts, repeated-evidence nudges) paint in the interactive
    /// transcript. These always reach the model as system messages; the
    /// transcript line is user-informational only, so `Balanced` and `Quiet`
    /// keep it out while `Verbose` and headless output keep it.
    pub fn loop_notices_visible(&self) -> bool {
        use vtcode_commons::ui_protocol::CommunicationStyle;
        !self.writes_to_inline_sink() || matches!(self.communication_style(), CommunicationStyle::Verbose)
    }

    /// Whether a tool result's attention rows (inline `⚠ warning` lines) paint
    /// for this tool call. These are outcome-adjacent signals rather than
    /// failure evidence, so command tools keep them under the default style;
    /// failed-result evidence uses [`Self::failure_evidence_visible`] instead.
    pub fn inline_failure_extras_visible(&self, is_command_tool: bool) -> bool {
        use vtcode_commons::ui_protocol::CommunicationStyle;
        match self.communication_style.resolve() {
            CommunicationStyle::Verbose => true,
            CommunicationStyle::Balanced => is_command_tool,
            // `resolve` maps `Unknown` to `Balanced`; the arm below only
            // keeps the decision total.
            CommunicationStyle::Quiet | CommunicationStyle::Unknown => false,
        }
    }

    /// Whether per-call tool transitions render in compact (grouped) form.
    ///
    /// Single source of truth for the compact-vs-expanded decision so every
    /// call site agrees; the stored mode is already normalized to
    /// `Compact`/`Expanded` by [`Self::set_tool_display_mode`].
    pub fn is_compact_display(&self) -> bool {
        self.tool_display_mode == ToolDisplayMode::Compact
    }

    pub fn set_diff_preview_mode(&mut self, mode: vtcode_commons::ui_protocol::DiffPreviewMode) {
        self.diff_preview_mode = mode;
    }

    /// Attach a completed-edit review payload so the TUI can expand on demand.
    pub fn record_diff_review(&self, anchor: vtcode_commons::ui_protocol::DiffReviewAnchor) {
        if let Some(sink) = &self.sink {
            sink.handle.record_diff_review(anchor);
        }
    }

    pub fn diff_preview_mode(&self) -> vtcode_commons::ui_protocol::DiffPreviewMode {
        self.diff_preview_mode
    }

    pub fn toggle_tool_display_mode(&mut self) -> ToolDisplayMode {
        self.flush_compact_command_group();
        let next = match self.tool_display_mode {
            ToolDisplayMode::Expanded | ToolDisplayMode::Unknown => ToolDisplayMode::Compact,
            ToolDisplayMode::Compact => ToolDisplayMode::Expanded,
        };
        self.tool_display_mode = next;
        next
    }
}
