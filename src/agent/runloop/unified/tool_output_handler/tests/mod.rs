pub(super) use super::*;
pub(super) use std::io::{IsTerminal, stdin};
pub(super) use std::sync::Arc;
pub(super) use tempfile::TempDir;
pub(super) use tokio::sync::{RwLock, mpsc::unbounded_channel};
pub(super) use vtcode_core::acp::ToolPermissionCache;
pub(super) use vtcode_core::config::loader::VTCodeConfig;
pub(super) use vtcode_core::core::decision_tracker::DecisionTracker;
pub(super) use vtcode_core::core::trajectory::TrajectoryLogger;
pub(super) use vtcode_core::tools::ApprovalRecorder;
pub(super) use vtcode_core::tools::registry::{ToolExecutionError, ToolRegistry};
pub(super) use vtcode_core::tools::result_cache::{ToolCacheKey, ToolResultCache};
pub(super) use vtcode_core::ui::inline_theme_from_core_styles;
pub(super) use vtcode_core::ui::theme;
pub(super) use vtcode_ui::tui::app::{InlineCommand, InlineHandle, SessionOptions, spawn_session_with_options};

fn build_harness_state() -> crate::agent::runloop::unified::run_loop_context::HarnessTurnState {
    crate::agent::runloop::unified::run_loop_context::HarnessTurnState::new(
        crate::agent::runloop::unified::run_loop_context::TurnRunId("test-run".to_string()),
        crate::agent::runloop::unified::run_loop_context::TurnId("test-turn".to_string()),
        4,
        60,
        0,
    )
}

fn dummy_handle() -> InlineHandle {
    InlineHandle::new_for_tests(unbounded_channel().0)
}

fn tracker_texts(lines: &[TrackerLine]) -> Vec<&str> {
    lines.iter().map(|line| line.text.as_str()).collect()
}

fn plain_tracker_lines(texts: &[&str]) -> Vec<TrackerLine> {
    texts.iter().map(|text| TrackerLine::plain((*text).to_string())).collect()
}

mod compact_command;
mod compact_groups;
mod compact_pty;
mod mcp_spool;
mod pipe_output_lines;
mod pipe_streams;
mod pipeline_collect;
mod row_render;
mod tracker_blocks;
