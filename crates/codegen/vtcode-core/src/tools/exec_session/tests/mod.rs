pub(super) use hashbrown::HashMap;
pub(super) use std::sync::Arc;
pub(super) use std::sync::atomic::{AtomicUsize, Ordering};
pub(super) use tempfile::tempdir;
pub(super) use tokio::time::{Duration, timeout};

pub(super) use super::{
    BackgroundShortcutResult, EXEC_SESSION_COMPLETION_COMMAND_MAX_BYTES, EXEC_SESSION_PREVIEW_HEAD_BYTES,
    EXEC_SESSION_PREVIEW_TAIL_BYTES, ExecSessionManager, MAX_BACKGROUND_PROCESSES, RetainedSessionPreview,
    bounded_completion_command,
};
pub(super) use crate::config::PtyConfig;
pub(super) use crate::tools::pty::{PtyCloseMode, PtySize};
pub(super) use crate::tools::registry::PtySessionManager;
pub(super) use crate::utils::path::canonicalize_workspace;

mod background_completion;
mod background_slots;
mod monitor;
mod pipe_buffer;
mod session_basics;
mod terminate;
mod ui_foreground;
mod watcher_close;
