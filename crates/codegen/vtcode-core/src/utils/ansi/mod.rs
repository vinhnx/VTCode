use std::io;

use anstream::AutoStream;

use crate::config::ToolDisplayMode;
use crate::config::loader::SyntaxHighlightingConfig;
use crate::utils::ansi_capabilities::AnsiCapabilities;
use vtcode_commons::ui_protocol::{CompactActivityMetadata, ToolOutputId};

pub use crate::utils::message_style::MessageStyle;

mod compact;
mod core;
mod links;
mod markdown;
mod output;
mod sink;

use sink::InlineSink;

pub use links::apply_file_opener_config;
use links::{
    make_clickable_target, should_strip_inline_local_link_underline, terminal_table_content_width,
    transcript_table_frame_width,
};
use sink::{contains_markdown_fence, looks_like_diff};

/// Canonical diagnostic persistence callback installed by the runtime.
pub use vtcode_commons::diagnostics::DiagnosticSink;

/// Renderer with deferred output buffering
pub struct AnsiRenderer {
    writer: AutoStream<io::Stdout>,
    buffer: String,
    color: bool,
    sink: Option<InlineSink>,
    last_line_was_empty: bool,
    highlight_config: SyntaxHighlightingConfig,
    capabilities: AnsiCapabilities,
    reasoning_visible: bool,
    screen_reader_mode: bool,
    show_diagnostics_in_transcript: bool,
    show_message_timestamps: bool,
    /// A completed agent response rendered this turn. The single wall-clock
    /// footer is emitted at turn end so intermediate responses stay unstamped.
    pending_response_footer: bool,
    /// Defer the wall-clock footer to the turn boundary.
    ///
    /// Armed by [`AnsiRenderer::begin_turn_message_timestamp`] for the duration
    /// of a turn. Outside a turn (session-exit reprint, pre-turn prompt replies)
    /// responses stamp immediately, so out-of-turn callers never owe the
    /// renderer a flush to keep their clock.
    defer_turn_footer: bool,
    tool_display_mode: ToolDisplayMode,
    communication_style: vtcode_commons::ui_protocol::CommunicationStyle,
    diff_preview_mode: vtcode_commons::ui_protocol::DiffPreviewMode,
    compact_command_group: Option<CompactActivityMetadata>,
    next_compact_group_id: u64,
    pending_tool_output_anchor: Option<ToolOutputId>,
    /// Capture id for the next exec-session expand notice. Set by the tool
    /// output handler after recording the complete session capture; consumed
    /// when the bounded body emits its `click to expand` row.
    session_expand_anchor: Option<ToolOutputId>,
    /// Force the next stream body onto the exec-session dim/expand path.
    /// Needed for `unified_exec` follow-ups: the tool name alone cannot tell a
    /// session poll from a command launch.
    session_body: bool,
    diagnostic_sink: Option<DiagnosticSink>,
    diagnostic_history: std::collections::VecDeque<vtcode_commons::diagnostics::Diagnostic>,
}

#[cfg(test)]
mod tests;
