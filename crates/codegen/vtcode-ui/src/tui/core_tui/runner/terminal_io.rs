use std::io::{self, Write};
use std::time::Duration;

use anyhow::Result;
use ratatui::{
    Terminal,
    backend::{Backend, ClearType as BackendClearType},
};

/// Mouse pointer shape states, mirroring standard text editor cursors.
///
/// Emitted as OSC 22 escape sequences, supported by xterm, kitty, foot,
/// WezTerm, iTerm2, and other modern terminal emulators.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MousePointerShape {
    #[default]
    Default,
    /// Hand cursor when hovering clickable links/files/URLs.
    Pointer,
    /// I-beam cursor during text selection or when a selection is active.
    Text,
}

impl MousePointerShape {
    fn as_osc22_name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Pointer => "pointer",
            Self::Text => "text",
        }
    }
}

/// Set the mouse pointer shape via OSC 22.
pub(crate) fn set_mouse_pointer_shape(shape: MousePointerShape) {
    let name = shape.as_osc22_name();
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\x1b]22;{name}\x07");
    let _ = stderr.flush();
}

/// Reset the mouse pointer shape to the terminal default.
pub(crate) fn reset_mouse_pointer_shape() {
    set_mouse_pointer_shape(MousePointerShape::Default);
}

pub(super) fn prepare_terminal<B: Backend>(terminal: &mut Terminal<B>) -> Result<()> {
    let _terminal_lock = crate::tui::core_tui::panic_hook::lock_terminal_operations();
    if crate::tui::core_tui::panic_hook::is_restore_claimed() {
        return Ok(());
    }

    terminal
        .hide_cursor()
        .map_err(|e| anyhow::anyhow!("failed to hide inline cursor: {e}"))?;
    crate::tui::ui::tui::panic_hook::mark_terminal_modified();
    terminal
        .backend_mut()
        .clear_region(BackendClearType::All)
        .map_err(|e| anyhow::anyhow!("failed to clear inline terminal: {e}"))?;
    crate::tui::ui::tui::panic_hook::mark_terminal_modified();
    Ok(())
}

pub(super) fn finalize_terminal<B: Backend>(terminal: &mut Terminal<B>, use_alternate_screen: bool) -> Result<()> {
    let _terminal_lock = crate::tui::core_tui::panic_hook::lock_terminal_operations();
    if crate::tui::core_tui::panic_hook::is_restore_claimed() {
        return Ok(());
    }

    // Minimal final paint: show the cursor and flush. All mode restores
    // (line clear for `^C`, cursor style, mouse pointer, bracketed paste,
    // focus/mouse, keyboard flags, raw mode, alternate leave) live in the
    // canonical `restore_tui()` path so Ctrl+C exits emit each sequence once.
    // Emitting them here too doubled escape traffic and caused flicker/noise.
    terminal
        .show_cursor()
        .map_err(|e| anyhow::anyhow!("failed to show cursor after inline session: {e}"))?;
    // Terminal::clear() snapshots the cursor via CPR (ESC[6n) to restore it afterward, which
    // blocks ~2s once the event stream is shut down at exit. Clear the backend viewport
    // instead; restore_tui() restores the saved cursor position right after.
    // For inline sessions clearing All wipes the viewport and leaves a
    // full-screen white gap above the next prompt — the TUI already drew
    // inline and the transcript should remain as scrollback, so only
    // alternate-screen sessions need a clear.
    if use_alternate_screen {
        terminal
            .backend_mut()
            .clear_region(BackendClearType::All)
            .map_err(|e| anyhow::anyhow!("failed to clear inline terminal after session: {e}"))?;
    }
    terminal
        .flush()
        .map_err(|e| anyhow::anyhow!("failed to flush inline terminal after session: {e}"))?;
    Ok(())
}

/// Drain any pending crossterm events (e.g., resize, focus responses, or buffered keystrokes)
/// so they don't leak to the shell or interfere with next startup.
///
/// The first poll waits up to 10ms for an in-flight terminal response (e.g. a CPR reply
/// triggered by PopKeyboardEnhancementFlags). Subsequent polls are instant — once the
/// first response is consumed any additional buffered events are drained without further
/// delay. This eliminates the "5;1R" escape-code leak on TUI exit.
pub(crate) fn drain_terminal_events() {
    use ratatui::crossterm::event;
    #[cfg(vendored_crossterm)]
    drain_events_with(|wait| event::read_timeout(wait).map(|event| event.is_some()));
    #[cfg(not(vendored_crossterm))]
    {
        // Registry crossterm has no atomic timed read. Its bounded poll can
        // collect terminal replies into the reader without an unbounded read
        // or a detached reader that could steal input from the next session.
        let _ = event::poll(Duration::from_millis(10));
    }
}

#[cfg(any(vendored_crossterm, test))]
fn drain_events_with(mut read: impl FnMut(Duration) -> io::Result<bool>) {
    let deadline = std::time::Instant::now() + Duration::from_millis(20);
    // A continuously writing terminal must not keep restoration in raw mode.
    for index in 0..128 {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let wait = if index == 0 {
            remaining.min(Duration::from_millis(10))
        } else {
            Duration::ZERO
        };
        match read(wait) {
            Ok(true) => {}
            Ok(false) | Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::backend::{TestBackend, WindowSize};
    use ratatui::buffer::Cell;
    use ratatui::layout::{Position, Size};

    use super::*;

    #[test]
    fn terminal_drain_stops_at_event_bound_and_empty_or_failed_reads() {
        let mut reads = 0;
        drain_events_with(|_| {
            reads += 1;
            Ok(true)
        });
        assert_eq!(reads, 128);
        let mut waits = Vec::new();
        drain_events_with(|wait| {
            waits.push(wait);
            Ok(waits.len() < 3)
        });
        assert_eq!(waits.len(), 3);
        assert!(waits[0] <= Duration::from_millis(10));
        assert_eq!(&waits[1..], &[Duration::ZERO, Duration::ZERO]);
        let mut errors = 0;
        drain_events_with(|_| {
            errors += 1;
            Err(io::Error::other("reader failed"))
        });
        assert_eq!(errors, 1);
    }

    /// Backend that fails cursor-position reads, mimicking a live terminal after the
    /// crossterm event reader has been shut down (the exit path in run_tui).
    struct NoCprBackend(TestBackend);

    impl Backend for NoCprBackend {
        type Error = io::Error;

        fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = (u16, u16, &'a Cell)>,
        {
            self.0.draw(content).map_err(|never| match never {})
        }

        fn hide_cursor(&mut self) -> Result<(), Self::Error> {
            self.0.hide_cursor().map_err(|never| match never {})
        }

        fn show_cursor(&mut self) -> Result<(), Self::Error> {
            self.0.show_cursor().map_err(|never| match never {})
        }

        fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
            Err(io::Error::other("cursor position unavailable"))
        }

        fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
            self.0.set_cursor_position(position).map_err(|never| match never {})
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            self.0.clear().map_err(|never| match never {})
        }

        fn clear_region(&mut self, clear_type: BackendClearType) -> Result<(), Self::Error> {
            self.0.clear_region(clear_type).map_err(|never| match never {})
        }

        fn size(&self) -> Result<Size, Self::Error> {
            self.0.size().map_err(|never| match never {})
        }

        fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
            self.0.window_size().map_err(|never| match never {})
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.0.flush().map_err(|never| match never {})
        }
    }

    #[test]
    fn finalize_terminal_does_not_query_cursor_position() {
        let mut terminal = Terminal::new(NoCprBackend(TestBackend::new(80, 24))).unwrap();
        assert!(finalize_terminal(&mut terminal, true).is_ok());
        let mut terminal = Terminal::new(NoCprBackend(TestBackend::new(80, 24))).unwrap();
        assert!(finalize_terminal(&mut terminal, false).is_ok());
    }
}
