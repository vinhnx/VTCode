//! Configuration types for tool output rendering.
//!
//! This module contains the `StreamRenderContext` struct that bundles
//! shared rendering parameters, reducing the 12-parameter signature
//! of `render_stream_section` to a single context parameter.

use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_tui_runtime::utils::ansi::{AnsiRenderer, MessageStyle};

use super::styles::{GitStyles, LsStyles};

/// Configuration for tool output rendering.
///
/// Bundles all parameters needed for stream rendering to reduce
/// function parameter counts and improve testability.
pub struct StreamRenderContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub mode: ToolOutputMode,
    pub tail_limit: usize,
    pub tool_name: Option<&'a str>,
    pub git_styles: &'a GitStyles,
    pub ls_styles: &'a LsStyles,
    pub fallback_style: MessageStyle,
    pub allow_ansi: bool,
    pub disable_spool: bool,
    pub config: Option<&'a VTCodeConfig>,
}

impl<'a> StreamRenderContext<'a> {
    /// Create a new context from individual parameters.
    pub fn new(
        renderer: &'a mut AnsiRenderer,
        mode: ToolOutputMode,
        tail_limit: usize,
        tool_name: Option<&'a str>,
        git_styles: &'a GitStyles,
        ls_styles: &'a LsStyles,
        fallback_style: MessageStyle,
        allow_ansi: bool,
        disable_spool: bool,
        config: Option<&'a VTCodeConfig>,
    ) -> Self {
        Self {
            renderer,
            mode,
            tail_limit,
            tool_name,
            git_styles,
            ls_styles,
            fallback_style,
            allow_ansi,
            disable_spool,
            config,
        }
    }
}
