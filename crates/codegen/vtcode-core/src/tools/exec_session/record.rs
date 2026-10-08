//! Execution backends: launch modes, completion commands, session records.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecSessionBackend {
    Pipe,
    Pty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecSessionLaunchMode {
    Foreground,
    UserBackground,
    ManagedBackground,
}

impl ExecSessionLaunchMode {
    pub(crate) fn is_background(self) -> bool {
        !matches!(self, Self::Foreground)
    }

    pub(crate) fn reserves_background_slot(self) -> bool {
        matches!(self, Self::UserBackground)
    }

    fn shows_in_background_drawer(self) -> bool {
        matches!(self, Self::UserBackground)
    }

    pub(crate) fn sets_foreground_session(self) -> bool {
        matches!(self, Self::Foreground)
    }
}

pub(crate) fn bounded_completion_command(command: &str) -> String {
    let without_controls: String = command
        .chars()
        .map(|character| if character.is_control() { ' ' } else { character })
        .collect();
    let collapsed = vtcode_commons::formatting::collapse_whitespace(&without_controls);
    let redacted = vtcode_commons::sanitizer::redact_secrets(collapsed);
    if redacted.len() <= EXEC_SESSION_COMPLETION_COMMAND_MAX_BYTES {
        return redacted;
    }
    vtcode_commons::formatting::truncate_byte_budget(
        &redacted,
        EXEC_SESSION_COMPLETION_COMMAND_MAX_BYTES.saturating_sub(3),
        "...",
    )
}

/// Bounded data used by the Local Agents drawer for one raw command session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecSessionUiSnapshot {
    pub metadata: VTCodeExecSession,
    pub preview: String,
    pub termination_requested: bool,
    pub updated_at: chrono::DateTime<Utc>,
}

/// Notification emitted after a background process has been confirmed exited.
///
/// The watcher owns the terminal transition, so a record emits at most one
/// notification even if multiple lifecycle checks observe the same exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecSessionCompletionEvent {
    /// Stable exec-session identity.
    pub session_id: ExecSessionId,
    /// Shell command associated with the session.
    pub command: String,
    /// Whether the session belongs to the managed subagent background flow.
    /// User-launched background exec sessions are delivered directly to the
    /// parent run loop; managed sessions are forwarded by their controller.
    pub managed_background: bool,
    /// Whether the runtime intentionally terminated the session.
    pub termination_requested: bool,
    /// Confirmed process exit status.
    pub exit_code: i32,
}

pub(crate) struct ExecSessionRecord {
    pub(crate) metadata: VTCodeExecSession,
    pub(crate) backend: ExecSessionBackend,
    _pty_guard: Option<PtySessionGuard>,
    pub(crate) foreground_pty_counter: ParkingMutex<Option<Arc<AtomicUsize>>>,
    pub(crate) background: AtomicBool,
    pub(crate) background_promoted_from_foreground: AtomicBool,
    pub(crate) show_in_background_drawer: AtomicBool,
    pub(crate) background_slot_reserved: AtomicBool,
    pub(crate) background_completion_published: AtomicBool,
    pub(crate) termination_requested: AtomicBool,
    pub(crate) completed_at: ParkingMutex<Option<chrono::DateTime<Utc>>>,
    preview: ParkingMutex<SessionPreviewState>,
    pub(crate) output_read_lock: Mutex<()>,
    pub(crate) background_watch: ParkingMutex<Option<JoinHandle<()>>>,
    pub(crate) foreground_watch: ParkingMutex<Option<JoinHandle<()>>>,
}

impl ExecSessionRecord {
    pub(crate) fn new(
        metadata: VTCodeExecSession,
        backend: ExecSessionBackend,
        pty_guard: Option<PtySessionGuard>,
        launch_mode: ExecSessionLaunchMode,
        background_slot_reserved: bool,
    ) -> Self {
        Self {
            metadata,
            backend,
            _pty_guard: pty_guard,
            foreground_pty_counter: ParkingMutex::new(None),
            background: AtomicBool::new(launch_mode.is_background()),
            background_promoted_from_foreground: AtomicBool::new(false),
            show_in_background_drawer: AtomicBool::new(launch_mode.shows_in_background_drawer()),
            background_slot_reserved: AtomicBool::new(background_slot_reserved),
            background_completion_published: AtomicBool::new(false),
            termination_requested: AtomicBool::new(false),
            completed_at: ParkingMutex::new(None),
            preview: ParkingMutex::new(SessionPreviewState::default()),
            output_read_lock: Mutex::new(()),
            background_watch: ParkingMutex::new(None),
            foreground_watch: ParkingMutex::new(None),
        }
    }

    pub(crate) fn remember_output(&self, output: Option<&str>, drain: bool) {
        let Some(output) = output else {
            return;
        };

        let mut preview = self.preview.lock();
        if drain {
            preview.retained.append(output);
            preview.pending = None;
        } else {
            preview.pending = Some(output.to_string());
        }
    }

    pub(crate) fn preview(&self) -> String {
        let preview = self.preview.lock();
        let mut retained = preview.retained.clone();
        if let Some(pending) = preview.pending.as_deref() {
            retained.append(pending);
        }
        retained.render()
    }
}
