use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use hashbrown::HashMap;
use parking_lot::Mutex as ParkingMutex;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Notify, RwLock, broadcast, watch};
use tokio::task::JoinHandle;
#[cfg(windows)]
use vtcode_bash_runner::GracefulTerminationResult;
use vtcode_bash_runner::{
    PipeSpawnOptions, PipeStdinMode, ProcessHandle, graceful_kill_process_group_default_async,
    spawn_pipe_process_with_options,
};

use crate::sandboxing::build_sanitized_env;
use crate::tools::ExecSessionId;
use crate::tools::output_spooler::{SpoolIntegrity, SpoolLineCounter, encode_digest_hex};
use crate::tools::pty::{PtyCloseMode, PtySize};
use crate::tools::registry::{PtySessionGuard, PtySessionManager};
use crate::tools::types::VTCodeExecSession;
use crate::utils::path::{canonicalize_workspace, ensure_path_within_workspace};
use crate::zsh_exec_bridge::ZshExecBridgeSession;

const PIPE_OUTPUT_HEAD_BYTES: usize = 8 * 1024;
const PIPE_OUTPUT_TAIL_BYTES: usize = 8 * 1024;
const EXEC_SESSION_PREVIEW_HEAD_BYTES: usize = 8 * 1024;
const EXEC_SESSION_PREVIEW_TAIL_BYTES: usize = 8 * 1024;
const EXEC_SESSION_COMPLETION_COMMAND_MAX_BYTES: usize = 512;
const EXEC_SESSION_COMPLETION_DRAIN_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(1);
const EXEC_SESSION_COMPLETION_DRAIN_POLL: tokio::time::Duration = tokio::time::Duration::from_millis(15);
/// Upper bound for watcher abort + backend close. Must cover the sum of inner
/// bounds (2s watch abort x2 + 2s child reap + 5s reader join) plus margin so
/// the outer timeout is not spurious while `spawn_blocking` finishes residual work.
const EXEC_SESSION_CLOSE_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(12);
/// Budget for aborting a lifecycle watcher before abandoning it.
const EXEC_SESSION_WATCH_ABORT_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(1);
/// Budget for acquiring the output-read lock during close. An in-flight peek
/// that never releases must not park close (and therefore the runloop).
const EXEC_SESSION_OUTPUT_READ_LOCK_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(1);

/// A missing runtime handle does not establish whether its command failed.
#[derive(Debug, thiserror::Error)]
#[error(
    "exec session '{session_id}' not found in this runtime. Call `write_stdin` with the exact `session_id` copied from the original run response (`next_wait_args.session_id` or `next_continue_args` `s`/`session_id`); never guess the id. If a background completion or turn diagnostics already recorded output for this id, reuse its output. Missing session state does not prove failure; rerun only if fresh execution is still needed."
)]
pub(crate) struct ExecSessionNotFound {
    pub(crate) session_id: crate::types::CompactStr,
}

fn missing_exec_session_error(session_id: &str) -> anyhow::Error {
    ExecSessionNotFound { session_id: session_id.into() }.into()
}

/// Maximum number of live background command sessions owned by one runtime.
pub const MAX_BACKGROUND_PROCESSES: usize = 3;

pub use manager::ExecSessionManager;
pub use output_buffer::BackgroundShortcutResult;
pub use record::{ExecSessionBackend, ExecSessionCompletionEvent, ExecSessionUiSnapshot};

mod manager;
mod output_buffer;
mod pipe;
mod record;

pub(crate) use manager::*;
pub(crate) use output_buffer::*;
pub(crate) use pipe::*;
pub(crate) use record::*;

#[cfg(all(test, unix))]
mod tests;
