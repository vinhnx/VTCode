use std::fs::OpenOptions;
use std::io::{self, Write};
use std::sync::atomic::Ordering;

use ratatui::crossterm::{
    cursor::{MoveToColumn, RestorePosition, SetCursorStyle, Show},
    event::{DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, PopKeyboardEnhancementFlags},
    execute,
    terminal::{Clear, ClearType, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, is_raw_mode_enabled},
};

use super::state::{self, COLOR_SCHEME_REPORTS_ENABLED, KEYBOARD_ENHANCEMENTS_PUSHED};

/// Emit the session's original iTerm2 profile switch-back, if one is pending.
///
/// VT Code switches to its `VT Code` icon profile at TUI startup; `OSC 1337;
/// SetProfile=` is sticky, so the matching switch back to the session's
/// original profile is emitted here (on normal exit, Ctrl+C, panic, and
/// SIGTERM) rather than left to iTerm2's Automatic Profile Switching, which
/// requires Shell Integration and is not reliably present.
fn emit_iterm2_profile_restore(writer: &mut impl Write, original: &str) -> io::Result<()> {
    writer.write_all(vtcode_commons::ansi_codes::set_iterm2_profile(original).as_bytes())
}

/// Emit the terminal-restoration escape sequence.
///
/// When `clear_alternate` is true we are currently on the alternate screen
/// buffer and should purge that buffer's contents before leaving it so no
/// transcript frames are accidentally revealed in the main scrollback.
/// When false we are inline on the main screen and must not emit a full
/// Clear(All) which would wipe the scrollback and leave a blank gap above
/// the next prompt.
///
/// Writes through any `Write` so the exact sequence is unit-testable.
fn emit_restore_sequence(writer: &mut impl Write, clear_alternate: bool) -> io::Result<()> {
    // Clear current line to remove any echoed ^C characters
    execute!(writer, MoveToColumn(0), Clear(ClearType::CurrentLine))?;

    // If we're on the alternate screen, clear its contents BEFORE leaving
    // so the act of leaving the buffer does not reveal the last TUI frame
    // as scrollback in the main terminal.
    if clear_alternate {
        if let Err(error) = execute!(writer, Clear(ClearType::All)) {
            tracing::debug!(%error, "failed to clear alternate terminal buffer before restore");
        }
    }

    // Leave alternate screen (best-effort regardless of current state)
    execute!(writer, LeaveAlternateScreen)?;

    // Disable terminal modes
    execute!(writer, DisableBracketedPaste)?;
    execute!(writer, DisableFocusChange)?;
    execute!(writer, DisableMouseCapture)?;

    // Only pop keyboard enhancement flags if actually pushed
    if KEYBOARD_ENHANCEMENTS_PUSHED.swap(false, Ordering::SeqCst) {
        execute!(writer, PopKeyboardEnhancementFlags)?;
    }

    // Only disable color-scheme reports if actually enabled; the flag is only
    // set on unix where the TUI turns Contour palette reports on.
    if COLOR_SCHEME_REPORTS_ENABLED.swap(false, Ordering::SeqCst) {
        writer.write_all(vtcode_commons::ansi_codes::COLOR_SCHEME_REPORTS_DISABLE.as_bytes())?;
    }

    Ok(())
}

fn open_tty_writer() -> Option<std::fs::File> {
    OpenOptions::new().write(true).open("/dev/tty").ok()
}

fn emit_restore_to_all_targets(clear_alternate: bool) -> Option<io::Error> {
    let mut first_error: Option<io::Error> = None;
    let profile_restore = state::take_iterm2_profile_to_restore();

    let mut stderr = io::stderr();
    crate::tui::core_tui::program_status::cleanup_before_restore(&mut stderr);
    if let Err(error) = emit_restore_sequence(&mut stderr, clear_alternate) {
        first_error.get_or_insert(error);
    }
    if let Err(error) = execute!(stderr, SetCursorStyle::DefaultUserShape, Show, RestorePosition) {
        first_error.get_or_insert_with(|| io::Error::other(error.to_string()));
    }
    // Clear the terminal title on the canonical stream as well: the emergency
    // Ctrl+C/SIGTERM paths never reach `Session::clear_terminal_title()`, so
    // without this the previous session title leaks into the shell.
    let _ = write!(stderr, "\x1b]0;default\x07");
    if let Some(original) = profile_restore.as_deref() {
        let _ = emit_iterm2_profile_restore(&mut stderr, original);
    }
    let _ = stderr.flush();
    crate::tui::core_tui::runner::terminal_io::reset_mouse_pointer_shape();

    if let Some(mut tty) = open_tty_writer() {
        let _ = emit_restore_sequence(&mut tty, clear_alternate);
        let _ = execute!(tty, SetCursorStyle::DefaultUserShape, Show, RestorePosition);
        let _ = tty.flush();
        let _ = write!(tty, "\x1b]22;default\x07");
        let _ = write!(tty, "\x1b]0;default\x07");
        if let Some(original) = profile_restore.as_deref() {
            let _ = emit_iterm2_profile_restore(&mut tty, original);
        }
        let _ = tty.flush();
    }

    // Stdout carries no escape sequences here, but flushing it guarantees any
    // buffered postamble (exit summary) is ordered after the restore instead
    // of interleaving with it on Ctrl+C exits.
    let _ = io::stdout().flush();

    first_error
}

/// Best-effort raw-mode release for post-TUI stdout postambles.
///
/// Unlike [`restore_tui()`] (escape restore is one-shot via `RESTORE_DONE`),
/// this always attempts `disable_raw_mode()` so a late `println!` never
/// staircases when output processing (`ONLCR`) is still off. Safe to call when
/// raw mode was never enabled: crossterm's disable is a no-op in that case.
/// Used after [`finish_deferred_raw_mode_restore`] as a force-cooked backstop.
pub fn ensure_raw_mode_disabled() {
    let _ = disable_raw_mode();
}

/// Drains terminal input that arrived after the TUI stopped reading it.
///
/// Teardown takes seconds (MCP shutdown, session-end hooks, TUI join) and the
/// event stream is cancelled long before that finishes. A late reply — most
/// visibly the kitty-protocol key-*release* report for the Ctrl+C that
/// triggered the exit (`CSI 99;5:3u`) — then sits in the tty input buffer, is
/// echoed once the tty returns to cooked mode, and spills into the shell after
/// the process exits. Draining after raw mode is restored (and once more just
/// before process exit) consumes it. Idempotent and bounded (~10 ms).
pub fn drain_pending_terminal_input() {
    crate::tui::core_tui::runner::terminal_io::drain_terminal_events();
}

/// Restore terminal to a usable state after a panic or error.
///
/// Escape-sequence restore is one-shot via `RESTORE_DONE`. The cooked-mode
/// transition is claimed separately, so this always finishes raw-mode restore
/// even when a prior [`restore_tui_keep_raw_mode`] already tore the TUI down.
///
/// - Drains pending events before and after restoration
/// - Clears the alternate viewport before leaving the alternate screen
/// - Leaves the alternate screen
/// - Preserves inline transcript scrollback instead of clearing the main viewport
/// - Disables bracketed paste, focus change, mouse capture
/// - Pops keyboard enhancement flags if pushed
/// - Resets cursor style and shows cursor
/// - Restores raw mode to its state before the TUI started
pub fn restore_tui() -> io::Result<()> {
    let error = restore_terminal_state(false);
    // Always force the cooked-mode transition. A prior `restore_tui_keep_raw_mode`
    // may have claimed the escape-sequence restore and left raw mode on; Drop
    // backstops, panic hooks, and emergency exits must never leave the tty raw.
    finish_raw_mode_restore();
    drain_pending_terminal_input();
    error
}

/// Restore every escape-sequence mode but deliberately **keep raw mode**.
///
/// Used by the runner's mode guard, which fires the moment the TUI task ends —
/// before the runloop has finished teardown (archive write, MCP shutdown, TUI
/// join). Raw mode keeps the tty's echo off, so a late kitty-protocol reply
/// (the key-release report for the exiting Ctrl+C) cannot be echoed onto the
/// screen; the exit postamble drains and then transitions to cooked mode as its
/// final step, in [`finish_deferred_raw_mode_restore`]. Every other restore path
/// goes through [`restore_tui`], which forces the cooked transition even when
/// the escape-sequence restore was already claimed, so a panic or emergency
/// exit can never leave the tty in raw mode.
pub fn restore_tui_keep_raw_mode() -> io::Result<()> {
    restore_terminal_state(true)
}

/// Restore raw mode to the state recorded before the TUI started.
///
/// If the terminal's current raw-mode can be queried we only toggle when needed;
/// otherwise fall back to disabling raw mode to preserve a conservative and
/// usable state.
fn restore_raw_mode_state() -> Option<io::Error> {
    let previous_raw = state::is_raw_mode_was_enabled();
    let mut error = None;
    match is_raw_mode_enabled() {
        Ok(current_enabled) => {
            if previous_raw && !current_enabled {
                if let Err(err) = enable_raw_mode() {
                    error = Some(err);
                }
            } else if !previous_raw && current_enabled {
                if let Err(err) = disable_raw_mode() {
                    error = Some(err);
                }
            }
        }
        Err(_) => {
            if !previous_raw {
                if let Err(err) = disable_raw_mode() {
                    error = Some(err);
                }
            }
        }
    }
    error
}

/// Finish a deferred raw-mode restore as the last act of a graceful exit.
///
/// Drains pending input *before* leaving raw mode — echo is still off, so the
/// bytes are consumed instead of echoed — then restores the tty's raw-mode
/// state and drains once more so nothing spills into the shell.
pub fn finish_deferred_raw_mode_restore() {
    drain_pending_terminal_input();
    finish_raw_mode_restore();
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    drain_pending_terminal_input();
}

/// Restore raw mode to its pre-TUI state exactly once.
///
/// Claimed separately from the escape-sequence restore so force paths
/// ([`restore_tui`]) can still finish the cooked transition after
/// [`restore_tui_keep_raw_mode`] already tore the TUI down.
fn finish_raw_mode_restore() {
    if !state::try_claim_raw_mode_restore() {
        return;
    }
    if let Some(error) = restore_raw_mode_state() {
        tracing::debug!(%error, "failed to restore raw mode");
    }
    state::mark_raw_mode_was_enabled(false);
}

/// Shared restore body. With `keep_raw_mode` the tty is left in raw mode and
/// the post-restore input drain is skipped (the caller owns that ordering).
fn restore_terminal_state(keep_raw_mode: bool) -> io::Result<()> {
    if !state::try_claim_restore() {
        return Ok(());
    }

    let _terminal_lock = state::lock_terminal_operations();
    state::mark_tui_deinitialized();

    let terminal_modified = state::is_terminal_modified();
    let alternate_active = state::is_alternate_screen_active();

    // Never emit restore sequences when no component modified the terminal
    // and no alternate screen is active: error reports for non-TUI runs
    // (failed startup, one-shot commands) would otherwise spray raw escape
    // sequences before the message. If alternate screen is active we must
    // still leave it even when TERMINAL_MODIFIED was not yet set (partial
    // init failure that succeeded to enter alt screen but failed before
    // flagging modification).
    if !terminal_modified && !alternate_active {
        return Ok(());
    }
    state::mark_terminal_restored();
    if alternate_active {
        state::mark_alternate_screen_active(false);
    }

    let mut first_error: Option<io::Error> = None;

    crate::tui::core_tui::runner::terminal_io::drain_terminal_events();

    // If the TUI ran on the alternate screen, clear that buffer before
    // leaving so the transcript is not revealed as scrollback in the
    // primary screen. Inline sessions must not be cleared here.
    let clear_alternate = alternate_active;
    if let Some(error) = emit_restore_to_all_targets(clear_alternate) {
        first_error.get_or_insert(error);
    }

    // Drain terminal responses from restore sequences while raw mode still active
    crate::tui::core_tui::runner::terminal_io::drain_terminal_events();

    // A graceful exit may ask to keep raw mode past this point: the tty's echo
    // stays off while the runloop finishes teardown, so late input (the kitty
    // key-release report for the exiting Ctrl+C) cannot be echoed onto the
    // screen. The cooked transition is owned by `finish_raw_mode_restore` and
    // claimed separately, so a later force path can still finish it.
    if keep_raw_mode {
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
        return Ok(());
    }

    // Best-effort stty sane equivalent for /dev/tty when raw-mode toggles
    // succeeded but tty still has echo off due to cargo wrapping stderr.
    if let Some(mut tty) = open_tty_writer() {
        let _ = tty.flush();
    }
    // Ensure both streams are flushed after restore sequences so the shell
    // prompt and any exit postamble are ordered after them. The raw-mode
    // transition itself runs in `finish_raw_mode_restore` (called by
    // `restore_tui` / `finish_deferred_raw_mode_restore`).
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_restore_terminal_no_panic_when_not_initialized() {
        state::RESTORE_DONE.store(false, Ordering::SeqCst);
        state::RAW_MODE_RESTORE_DONE.store(false, Ordering::SeqCst);
        state::TUI_INITIALIZED.store(false, Ordering::SeqCst);

        let result = restore_tui();
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn keep_raw_mode_leaves_raw_restore_pending() {
        state::RESTORE_DONE.store(false, Ordering::SeqCst);
        state::RAW_MODE_RESTORE_DONE.store(false, Ordering::SeqCst);
        state::mark_terminal_modified();

        let _ = restore_tui_keep_raw_mode();
        assert!(state::RESTORE_DONE.load(Ordering::SeqCst), "escape restore must be claimed");
        assert!(
            !state::RAW_MODE_RESTORE_DONE.load(Ordering::SeqCst),
            "keep_raw_mode must leave the cooked transition pending"
        );

        let _ = restore_tui();
        assert!(
            state::RAW_MODE_RESTORE_DONE.load(Ordering::SeqCst),
            "force restore_tui after keep_raw_mode must finish the cooked transition"
        );
        state::mark_terminal_restored();
    }

    #[test]
    fn raw_mode_restore_is_claimed_exactly_once() {
        state::RAW_MODE_RESTORE_DONE.store(false, Ordering::SeqCst);
        assert!(state::try_claim_raw_mode_restore());
        assert!(!state::try_claim_raw_mode_restore(), "second raw restore claim must fail");
        state::RAW_MODE_RESTORE_DONE.store(false, Ordering::SeqCst);
    }

    #[test]
    fn finish_deferred_then_restore_tui_is_idempotent() {
        state::RESTORE_DONE.store(false, Ordering::SeqCst);
        state::RAW_MODE_RESTORE_DONE.store(false, Ordering::SeqCst);
        state::mark_terminal_modified();

        let _ = restore_tui_keep_raw_mode();
        finish_deferred_raw_mode_restore();
        assert!(state::RAW_MODE_RESTORE_DONE.load(Ordering::SeqCst));

        let _ = restore_tui();
        assert!(state::RAW_MODE_RESTORE_DONE.load(Ordering::SeqCst));
        state::mark_terminal_restored();
    }

    #[test]
    fn restore_sequence_does_not_clear_inline_screen() {
        let mut bytes: Vec<u8> = Vec::new();
        // Inline session: do not clear the full-screen buffer here
        emit_restore_sequence(&mut bytes, false).unwrap();
        let text = String::from_utf8(bytes).unwrap();

        assert!(!text.contains("\x1b[2J"), "inline restore must not emit ESC[2J, got: {text:?}");
        assert!(text.contains("\x1b[1G\x1b[2K"), "inline restore must keep line clear, got: {text:?}");
        assert!(
            text.contains("\x1b[?1049l"),
            "inline restore must leave alternate screen (best-effort), got: {text:?}"
        );
    }

    #[test]
    fn restore_sequence_clears_alternate_buffer_before_leaving() {
        let mut bytes: Vec<u8> = Vec::new();
        // Alternate session: clear the alternate buffer to avoid leaking
        // TUI frames into the main scrollback when leaving it.
        emit_restore_sequence(&mut bytes, true).unwrap();
        let text = String::from_utf8(bytes).unwrap();

        assert!(
            text.contains("\x1b[2J"),
            "alternate-restore must emit ESC[2J to purge alternate buffer, got: {text:?}"
        );
        let clear_index = text.find("\x1b[2J").expect("alternate screen clear is present");
        let leave_index = text.find("\x1b[?1049l").expect("alternate screen leave is present");
        assert!(clear_index < leave_index, "alternate buffer must be cleared before leaving: {text:?}");
    }

    #[test]
    fn alternate_screen_flag_resets_on_new_session() {
        state::mark_alternate_screen_active(true);
        assert!(state::is_alternate_screen_active());
        state::mark_tui_initialized();
        assert!(!state::is_alternate_screen_active(), "new TUI session must start inline");
    }

    #[test]
    fn terminal_modified_flag_round_trip() {
        state::mark_terminal_restored();
        assert!(!state::is_terminal_modified());
        state::mark_terminal_modified();
        assert!(state::is_terminal_modified());
        state::mark_terminal_restored();
        assert!(!state::is_terminal_modified());
    }

    #[test]
    fn new_tui_session_waits_for_terminal_mutation() {
        state::mark_terminal_restored();
        assert!(!state::is_terminal_modified());
        state::mark_tui_initialized();
        assert!(state::is_tui_initialized());
        assert!(!state::is_terminal_modified(), "TUI registration must not claim terminal mutation");
        state::mark_tui_deinitialized();
    }

    #[test]
    fn restore_skips_emission_when_terminal_never_modified() {
        state::RESTORE_DONE.store(false, Ordering::SeqCst);
        state::TUI_INITIALIZED.store(false, Ordering::SeqCst);
        state::mark_terminal_restored();

        let result = restore_tui();
        assert!(result.is_ok() || result.is_err());
        assert!(
            state::RESTORE_DONE.load(Ordering::SeqCst),
            "restore must still be claimed so later calls stay no-ops"
        );
        assert!(!state::is_terminal_modified(), "unmodified terminal must stay unmodified");

        // A modified terminal must still run the full restore and clear the flag.
        state::RESTORE_DONE.store(false, Ordering::SeqCst);
        state::mark_terminal_modified();
        let result = restore_tui();
        assert!(result.is_ok() || result.is_err());
        assert!(!state::is_terminal_modified(), "restore must clear the modified flag");
    }

    #[test]
    fn iterm2_profile_restore_emits_switch_back_sequence() {
        let mut bytes: Vec<u8> = Vec::new();
        emit_iterm2_profile_restore(&mut bytes, "Solarized Dark").unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "\x1b]1337;SetProfile=Solarized Dark\x07");

        let mut default_bytes: Vec<u8> = Vec::new();
        emit_iterm2_profile_restore(&mut default_bytes, "Default").unwrap();
        assert_eq!(String::from_utf8(default_bytes).unwrap(), "\x1b]1337;SetProfile=Default\x07");
    }
}
