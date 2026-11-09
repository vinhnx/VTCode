//! Single-shot Program Status Protocol (OSC 7501) reporting for headless exec.
//!
//! The interactive TUI owns the full session projection
//! (`vtcode_ui::tui::core_tui::program_status`). Exec runs only need one owned
//! record: `working` at start, then exactly one of `done`, `error`, or `idle`
//! (cancellation). There is no approval prompt under exec (`full_auto` is
//! always on), so `blocked` is never emitted here.

use std::io::{self, IsTerminal, Write};

use vtcode_commons::program_status::{ProgramState, encode_clear, encode_report, record_segment};

/// Pure emission gate, shared by the live path and the tests.
///
/// Reports go to stderr only, so only stderr needs to be a terminal (unlike
/// the TUI, which requires all three standard streams). JSON and quiet runs
/// stay silent to keep machine output and suppressed output byte-clean.
pub(super) fn should_emit_program_status(
    enabled: bool,
    quiet: bool,
    json: bool,
    stderr_terminal: bool,
    vte_ok: bool,
) -> bool {
    enabled && !quiet && !json && stderr_terminal && vte_ok
}

/// One owned exec record. Best effort: delivery failures are logged and the
/// run continues, since status is presentation, not execution evidence.
pub(super) struct ExecProgramStatus {
    id: String,
    active: bool,
}

impl ExecProgramStatus {
    /// Evaluate the live gate and emit `working` when active.
    pub(super) fn start(enabled: bool, quiet: bool, json: bool) -> Self {
        let active = should_emit_program_status(
            enabled,
            quiet,
            json,
            io::stderr().is_terminal(),
            vtcode_commons::program_status::vte_allows_unknown_osc(),
        );
        let origin = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let status = Self {
            id: record_segment("exec", &format!("{}-{origin}", std::process::id())),
            active,
        };
        status.emit(ProgramState::Working);
        status
    }

    /// Emit the terminal outcome: `done` on success, `error` otherwise. Both
    /// survive the following shell prompt by protocol design, so inboxes can
    /// show the finished run.
    pub(super) fn finish(&self, success: bool) {
        self.emit(if success {
            ProgramState::Done
        } else {
            ProgramState::Error
        });
    }

    /// Emit `idle` for user cancellation, per the protocol.
    pub(super) fn cancel(&self) {
        self.emit(ProgramState::Idle);
    }

    /// Encode the report for `state`, or `None` when this run is silent.
    /// Generic labels only: never prompts, commands, paths, or output text.
    fn wire(&self, state: ProgramState) -> Option<String> {
        if !self.active {
            return None;
        }
        // `clear` carries no content keys on any VT Code emission path.
        let result = if state == ProgramState::Clear {
            encode_clear(&self.id)
        } else {
            let message = match state {
                ProgramState::Working => "Working",
                ProgramState::Done => "Completed",
                ProgramState::Error => "Failed",
                ProgramState::Idle => "Idle",
                // Unreachable today (exec never reports blocked), kept
                // explicit so a future caller cannot silently inherit a
                // wrong label.
                ProgramState::Blocked => "Waiting for guidance",
                // Unused: Clear encodes content-free above; the arm exists
                // only for exhaustiveness.
                ProgramState::Clear => "Idle",
            };
            encode_report(&self.id, state, None, None, vtcode_config::constants::app::DISPLAY_NAME, message)
        };
        match result {
            Ok(wire) => Some(wire),
            Err(error) => {
                tracing::warn!(error = ?error, "exec program status encoding failed");
                None
            }
        }
    }

    fn emit(&self, state: ProgramState) {
        if let Some(wire) = self.wire(state) {
            let mut stderr = io::stderr().lock();
            if let Err(error) = stderr.write_all(wire.as_bytes()).and_then(|()| stderr.flush()) {
                tracing::warn!(error = ?error, "exec program status delivery failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active() -> ExecProgramStatus {
        ExecProgramStatus { id: "texec1".to_string(), active: true }
    }

    #[test]
    fn exec_gate_needs_every_condition() {
        assert!(should_emit_program_status(true, false, false, true, true));
        // Each gate input can veto emission on its own.
        assert!(!should_emit_program_status(false, false, false, true, true));
        assert!(!should_emit_program_status(true, true, false, true, true));
        assert!(!should_emit_program_status(true, false, true, true, true));
        assert!(!should_emit_program_status(true, false, false, false, true));
        assert!(!should_emit_program_status(true, false, false, true, false));
        assert!(!should_emit_program_status(false, true, true, false, false));
    }

    #[test]
    fn exec_wire_covers_start_finish_and_cancel() {
        let status = active();
        let working = status.wire(ProgramState::Working).expect("active run must encode");
        assert!(working.contains("state=working:"), "{working:?}");
        assert!(working.contains(":app=vtcode:"), "{working:?}");
        assert!(!working.contains("kind="), "{working:?}");
        assert!(!working.contains("progress="), "{working:?}");

        // Asymmetric outcomes: success and failure encode different states.
        let done = status.wire(ProgramState::Done).expect("active run must encode");
        let error = status.wire(ProgramState::Error).expect("active run must encode");
        assert!(done.contains("state=done:"), "{done:?}");
        assert!(error.contains("state=error:"), "{error:?}");
        assert!(!done.contains("progress="), "{done:?}");
        assert!(!error.contains("progress="), "{error:?}");

        let idle = status.wire(ProgramState::Idle).expect("active run must encode");
        assert!(idle.contains("state=idle:"), "{idle:?}");
    }

    #[test]
    fn exec_wire_is_silent_when_inactive() {
        let status = ExecProgramStatus { id: "texec1".to_string(), active: false };
        for state in [
            ProgramState::Working,
            ProgramState::Done,
            ProgramState::Error,
            ProgramState::Idle,
            ProgramState::Blocked,
            ProgramState::Clear,
        ] {
            assert_eq!(status.wire(state), None, "{state:?} must stay silent");
        }
    }

    #[test]
    fn exec_wire_keeps_unemitted_states_consistent() {
        let status = active();
        // Blocked is never emitted (no approval waits under exec), but the
        // label stays TUI-consistent if that ever changes. Oracle computed
        // outside the code under test: base64("Waiting for guidance").
        let blocked = status.wire(ProgramState::Blocked).expect("active run must encode");
        assert!(blocked.contains("state=blocked:"), "{blocked:?}");
        assert!(!blocked.contains("kind="), "{blocked:?}");
        assert!(blocked.ends_with(":msg=V2FpdGluZyBmb3IgZ3VpZGFuY2U=\x1b\\"), "{blocked:?}");
        // Clear carries no content keys on any emission path.
        let clear = status.wire(ProgramState::Clear).expect("active run must encode");
        assert_eq!(clear, "\x1b]7501;state=clear:id=texec1:app=vtcode\x1b\\");
    }

    #[test]
    fn exec_id_stays_within_protocol_limits() {
        // Same construction as `start`: kind-scoped opaque segment.
        let id = record_segment("exec", "12345-67890");
        assert!(id.len() <= 32, "segment must fit the 32-byte id budget");
        assert!(encode_report(&id, ProgramState::Working, None, None, "VT Code", "Working").is_ok());
    }
}
