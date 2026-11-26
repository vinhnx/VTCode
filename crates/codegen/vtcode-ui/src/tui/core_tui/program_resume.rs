//! Session-owned terminal resume projection. Never controls input or execution.

use std::io::{self, IsTerminal, Write};

use vtcode_commons::program_resume::{
    ProgramResumeUpdate, current_pid, current_unix_time, encode_clear, encode_report, resume_token,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResumeTarget {
    argv: Vec<String>,
    cwd: Option<String>,
}

#[derive(Default)]
pub(crate) struct ProgramResume {
    enabled: bool,
    terminal: bool,
    target: Option<ResumeTarget>,
    reported: Option<ResumeTarget>,
}

/// Pure attach gate: all three standard streams must be a terminal, and
/// pre-0.54 VTE parsers stay silent on unknown OSC bytes (same as status).
fn should_attach_terminal(stdin: bool, stdout: bool, stderr: bool, vte_ok: bool) -> bool {
    stdin && stdout && stderr && vte_ok
}

impl ProgramResume {
    /// Set only by the real interactive runner, never a test/headless session.
    pub(crate) fn attach_terminal(&mut self) {
        self.terminal = should_attach_terminal(
            io::stdin().is_terminal(),
            io::stdout().is_terminal(),
            io::stderr().is_terminal(),
            vtcode_commons::program_status::vte_allows_unknown_osc(),
        );
    }

    pub(crate) fn apply(&mut self, update: ProgramResumeUpdate) {
        match update {
            ProgramResumeUpdate::Configure { enabled } => {
                self.enabled = enabled;
            }
            ProgramResumeUpdate::SetTarget { argv, cwd } => {
                // Validate at the boundary: only well-formed targets are
                // stored, so a bad sender warns once here instead of on every
                // flush. The previous target (if any) stays live.
                if let Err(error) = vtcode_commons::program_resume::validate_target(&argv, cwd.as_deref()) {
                    tracing::warn!(error = ?error, "ignoring invalid program resume target");
                    return;
                }
                self.target = Some(ResumeTarget { argv, cwd });
            }
            ProgramResumeUpdate::Clear => {
                self.target = None;
            }
        }
    }

    fn desired(&self) -> Option<&ResumeTarget> {
        if self.enabled { self.target.as_ref() } else { None }
    }

    /// Encode the pending change, if any. Pure: no terminal I/O.
    fn pending_wire(&self, pid: u32, time: u64, token: &str) -> Option<anyhow::Result<String>> {
        let desired = self.desired();
        if desired == self.reported.as_ref() {
            return None;
        }
        Some(match desired {
            Some(target) => encode_report(pid, time, &target.argv, target.cwd.as_deref(), token),
            None => encode_clear(pid, time, token),
        })
    }

    pub(crate) fn flush(&mut self) {
        if !self.terminal {
            return;
        }
        // The token gate doubles as feature detection: without it the
        // terminal does not implement OSC 7577, so there is nothing to send.
        let Some(token) = resume_token() else {
            return;
        };
        let Some(wire) = self.pending_wire(current_pid(), current_unix_time(), &token) else {
            return;
        };
        let wire = match wire {
            Ok(wire) => wire,
            Err(error) => {
                tracing::warn!(error = ?error, "terminal program resume encoding failed");
                return;
            }
        };
        let _guard = super::panic_hook::lock_terminal_operations();
        if super::panic_hook::is_restore_claimed() {
            return;
        }
        if let Err(error) = self.deliver(&mut io::stderr().lock(), &wire) {
            tracing::warn!(error = ?error, "terminal program resume delivery failed");
        }
    }

    /// Write one wire report and settle it. A failed write may still have
    /// reached the terminal, but both outcomes are idempotent (same report,
    /// or a repeated clear), so retaining the pending change for the next
    /// flush is safe.
    fn deliver(&mut self, writer: &mut impl Write, wire: &str) -> io::Result<()> {
        writer.write_all(wire.as_bytes())?;
        writer.flush()?;
        self.reported = self.desired().cloned();
        Ok(())
    }

    /// Re-emit the current target even when unchanged.
    ///
    /// Used when the program gets the terminal back after a suspend (SIGTSTP):
    /// another program may have overwritten the record while this one slept.
    pub(crate) fn reannounce(&mut self) {
        self.reported = None;
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_TOKEN: &str = "test-token-7577";

    fn target() -> ResumeTarget {
        ResumeTarget {
            argv: ["vtcode", "--workspace", "/tmp/ws", "--resume", "session-1"]
                .iter()
                .map(|part| part.to_string())
                .collect(),
            cwd: Some("/tmp/ws".to_string()),
        }
    }

    fn configured() -> ProgramResume {
        ProgramResume {
            enabled: true,
            terminal: true,
            ..ProgramResume::default()
        }
    }

    #[test]
    fn pending_wire_encodes_target_once_then_settles() {
        let mut resume = configured();
        let set = target();
        resume.apply(ProgramResumeUpdate::SetTarget { argv: set.argv.clone(), cwd: set.cwd.clone() });

        let wire = resume
            .pending_wire(4242, 1_800_000_000, TEST_TOKEN)
            .expect("new target must encode")
            .expect("valid target must encode");
        assert!(wire.starts_with("\x1b]7577;pid=4242:time=1800000000:argv="), "{wire:?}");
        assert!(wire.contains(":cwd="), "{wire:?}");
        assert!(wire.ends_with("\x1b\\"), "{wire:?}");
        // Oracle: base64 of the first two argv elements, computed outside the code under test.
        assert!(wire.contains("dnRjb2Rl"), "{wire:?}");
        assert!(wire.contains("LS13b3Jrc3BhY2U="), "{wire:?}");

        resume.reported = resume.desired().cloned();
        assert!(resume.pending_wire(4242, 1_800_000_001, TEST_TOKEN).is_none());
    }

    #[test]
    fn pending_wire_withdraws_on_clear_and_on_disable() {
        let mut resume = configured();
        let set = target();
        resume.apply(ProgramResumeUpdate::SetTarget { argv: set.argv.clone(), cwd: set.cwd.clone() });
        resume.reported = resume.desired().cloned();

        resume.apply(ProgramResumeUpdate::Clear);
        let wire = resume
            .pending_wire(4242, 1_800_000_000, TEST_TOKEN)
            .expect("clear must encode")
            .expect("clear must encode");
        assert!(wire.contains(":clear=1:mac="), "{wire:?}");

        // Disabling with an outstanding record withdraws it as well.
        resume.apply(ProgramResumeUpdate::SetTarget { argv: set.argv, cwd: set.cwd });
        resume.reported = resume.desired().cloned();
        resume.apply(ProgramResumeUpdate::Configure { enabled: false });
        let wire = resume
            .pending_wire(4242, 1_800_000_000, TEST_TOKEN)
            .expect("disable must withdraw")
            .expect("disable must encode");
        assert!(wire.contains(":clear=1:mac="), "{wire:?}");
    }

    #[test]
    fn pending_wire_rereports_on_target_change_and_reannounce() {
        let mut resume = configured();
        let set = target();
        resume.apply(ProgramResumeUpdate::SetTarget { argv: set.argv.clone(), cwd: set.cwd.clone() });
        resume.reported = resume.desired().cloned();

        resume.apply(ProgramResumeUpdate::SetTarget {
            argv: ["vtcode", "--resume", "session-2"]
                .iter()
                .map(|part| part.to_string())
                .collect(),
            cwd: None,
        });
        let wire = resume
            .pending_wire(4242, 1_800_000_000, TEST_TOKEN)
            .expect("changed target must encode")
            .expect("changed target must encode");
        assert!(wire.contains("argv="), "{wire:?}");
        assert!(!wire.contains(":cwd="), "{wire:?}");

        // Reannounce forces a re-emit of the unchanged target.
        resume.reported = resume.desired().cloned();
        assert!(resume.pending_wire(4242, 1_800_000_001, TEST_TOKEN).is_none());
        resume.reported = None;
        assert!(resume.pending_wire(4242, 1_800_000_002, TEST_TOKEN).is_some());
    }

    #[test]
    fn session_commands_route_to_resume_adapter() {
        use crate::tui::core_tui::{app::AppSession, app::types::InlineCommand, types::InlineTheme};

        let mut session = AppSession::new(InlineTheme::default(), None, 24);
        let set = target();
        session.handle_command(InlineCommand::ProgramResume(ProgramResumeUpdate::Configure { enabled: true }));
        session.handle_command(InlineCommand::ProgramResume(ProgramResumeUpdate::SetTarget {
            argv: set.argv.clone(),
            cwd: set.cwd.clone(),
        }));
        let resume = &session.core.program_resume;
        assert_eq!(resume.desired(), Some(&set));
        let wire = resume
            .pending_wire(4242, 1_800_000_000, TEST_TOKEN)
            .expect("routed target must encode")
            .expect("valid target must encode");
        assert!(wire.starts_with("\x1b]7577;"), "{wire:?}");

        session.handle_command(InlineCommand::ProgramResume(ProgramResumeUpdate::Clear));
        assert_eq!(session.core.program_resume.desired(), None);
    }

    #[test]
    fn attach_gate_needs_every_stream_and_vte() {
        assert!(should_attach_terminal(true, true, true, true));
        for gated in [
            (false, true, true, true),
            (true, false, true, true),
            (true, true, false, true),
            (true, true, true, false),
            (false, false, false, false),
        ] {
            assert!(!should_attach_terminal(gated.0, gated.1, gated.2, gated.3), "{gated:?}");
        }
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("boom"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("boom"))
        }
    }

    #[test]
    fn deliver_writes_exact_bytes_and_settles_only_on_success() {
        let mut resume = configured();
        let set = target();
        resume.apply(ProgramResumeUpdate::SetTarget { argv: set.argv.clone(), cwd: set.cwd.clone() });
        let wire = resume
            .pending_wire(4242, 1_800_000_000, TEST_TOKEN)
            .expect("pending")
            .expect("encodes");

        // Failure path first: nothing settles, the change stays pending.
        assert!(resume.deliver(&mut FailingWriter, &wire).is_err());
        assert_eq!(resume.reported, None);
        assert!(resume.pending_wire(4242, 1_800_000_001, TEST_TOKEN).is_some());

        // Success path: exact bytes land and the change settles.
        let mut output = Vec::new();
        resume.deliver(&mut output, &wire).expect("delivers");
        assert_eq!(output, wire.as_bytes());
        assert_eq!(resume.reported.as_ref(), Some(&set));
        assert!(resume.pending_wire(4242, 1_800_000_002, TEST_TOKEN).is_none());
    }

    #[test]
    fn invalid_targets_are_rejected_at_apply_and_keep_the_previous_target() {
        let mut resume = configured();
        resume.apply(ProgramResumeUpdate::SetTarget {
            argv: vec!["./relative".to_string()],
            cwd: Some("relative".to_string()),
        });
        assert_eq!(resume.desired(), None);
        assert!(resume.pending_wire(4242, 1_800_000_000, TEST_TOKEN).is_none());

        // A valid target stays live when a later invalid one arrives.
        let set = target();
        resume.apply(ProgramResumeUpdate::SetTarget { argv: set.argv.clone(), cwd: set.cwd.clone() });
        resume.apply(ProgramResumeUpdate::SetTarget { argv: vec![], cwd: None });
        assert_eq!(resume.desired(), Some(&set));
    }
}
