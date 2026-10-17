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
    tool_display_mode: ToolDisplayMode,
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
}

#[cfg(test)]
mod tests;
