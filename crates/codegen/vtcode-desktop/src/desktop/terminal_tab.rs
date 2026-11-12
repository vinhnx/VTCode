//! Retained state for one user-controlled terminal tab.

use std::path::Path;
use std::sync::mpsc::TryRecvError;

use anyhow::Result;
use gpui_kit::{Hsla, Rgba, component::theme::ThemeColor};
use portable_pty::PtySize;
use rio_vt::config::colors::{
    ColorRgb, Colors, NamedColor,
    term::{COUNT, List},
};
use rio_vt::crosswords::Mode;
use rio_vt::event::{RioEvent, WindowSize};

use super::terminal_process::{TerminalProcess, TerminalProcessEvent};
use super::terminal_screen::TerminalScreen;

const MAX_EVENTS_PER_DRAIN: usize = 64;
const SCROLLBACK_LINES: usize = 10_000;

pub(super) struct TerminalDrain {
    pub(super) changed: bool,
    pub(super) more: bool,
    pub(super) events: Vec<RioEvent>,
}

pub(super) fn palette_for_theme(colors: &ThemeColor) -> List {
    let mut palette = List::from(&Colors::default());
    let rgba = |color: Hsla| {
        let color: Rgba = color.into();
        [color.r, color.g, color.b, color.a]
    };
    palette[NamedColor::Foreground] = rgba(colors.foreground);
    palette[NamedColor::Background] = rgba(colors.background);
    palette[NamedColor::Cursor] = rgba(colors.caret);
    palette
}

pub(super) struct TerminalTab {
    process: TerminalProcess,
    size: PtySize,
    palette: List,
    pub(super) screen: TerminalScreen,
    pub(super) exit_status: Option<String>,
    pub(super) error: Option<String>,
    pub(super) output_closed: bool,
}

impl TerminalTab {
    pub(super) fn spawn(workspace: &Path, shell: &Path, size: PtySize, palette: List) -> Result<Self> {
        Self::with_process(TerminalProcess::spawn(workspace, shell, size)?, size, palette)
    }

    pub(super) fn spawn_login(workspace: &Path, shell: &Path, size: PtySize, palette: List) -> Result<Self> {
        Self::with_process(TerminalProcess::spawn_login(workspace, shell, size)?, size, palette)
    }

    fn with_process(process: TerminalProcess, size: PtySize, palette: List) -> Result<Self> {
        Ok(Self {
            process,
            size,
            palette,
            screen: TerminalScreen::new(size.cols, size.rows, SCROLLBACK_LINES),
            exit_status: None,
            error: None,
            output_closed: false,
        })
    }

    pub(super) fn set_palette(&mut self, palette: List) {
        self.palette = palette;
    }

    pub(super) fn display_rows(&self) -> Vec<Vec<super::terminal_screen::TerminalRowRun>> {
        self.screen.display_rows(&self.palette)
    }

    pub(super) fn app_cursor(&self) -> bool {
        self.screen.mode().contains(Mode::APP_CURSOR)
    }

    pub(super) fn grid_size(&self) -> PtySize {
        self.size
    }

    pub(super) fn select_all(&mut self) {
        self.screen.select_all();
    }

    pub(super) fn selected_text(&self) -> Option<String> {
        self.screen.selected_text()
    }

    /// Clear any selection, returning whether one existed.
    pub(super) fn clear_selection(&mut self) -> bool {
        let had_selection = self.screen.selection_range().is_some();
        self.screen.clear_selection();
        had_selection
    }

    pub(super) fn input(&self, bytes: Vec<u8>) -> Result<()> {
        self.process
            .input(bytes)
            .map_err(|error| anyhow::anyhow!("terminal input queue: {error}"))
    }

    pub(super) fn paste(&self, text: &str) -> Result<()> {
        self.input(self.screen.paste_bytes(text))
    }

    pub(super) fn resize(&mut self, mut size: PtySize) -> Result<()> {
        size.rows = size.rows.max(1);
        size.cols = size.cols.max(1);
        self.process
            .resize(size)
            .map_err(|error| anyhow::anyhow!("terminal resize queue: {error}"))?;
        let _events = self.screen.resize(size.cols, size.rows);
        self.size = size;
        Ok(())
    }

    /// Process a bounded batch, including output that arrives after shell exit.
    /// Returns whether state changed, another batch may be waiting, and view events.
    pub(super) fn drain(&mut self) -> TerminalDrain {
        let mut view_events = Vec::new();
        for processed in 0..MAX_EVENTS_PER_DRAIN {
            match self.process.try_recv() {
                Ok(TerminalProcessEvent::Output(bytes)) => {
                    for event in self.screen.process(&bytes) {
                        let reply = terminal_reply(&event, self.size, &self.screen, &self.palette);
                        if let Some(reply) = reply
                            && let Err(error) = self.process.input(reply.into_bytes())
                        {
                            self.error = Some(format!("Terminal reply: {error}"));
                        } else if may_forward_to_view(&event) {
                            view_events.push(event);
                        }
                    }
                }
                Ok(TerminalProcessEvent::OutputClosed) => self.output_closed = true,
                Ok(TerminalProcessEvent::Exited(status)) => self.exit_status = Some(status),
                Ok(TerminalProcessEvent::Error(error)) => self.error = Some(error),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                    return TerminalDrain {
                        changed: processed > 0,
                        more: false,
                        events: view_events,
                    };
                }
            }
        }
        TerminalDrain { changed: true, more: true, events: view_events }
    }
}

// Child output is untrusted. OSC clipboard operations must not bypass the
// user's explicit copy/paste actions or expose clipboard contents to a shell.
fn may_forward_to_view(event: &RioEvent) -> bool {
    !matches!(
        event,
        RioEvent::PtyWrite(..)
            | RioEvent::TextAreaSizeRequest(..)
            | RioEvent::ColorRequest(..)
            | RioEvent::ClipboardStore(..)
            | RioEvent::ClipboardLoad(..)
    )
}

fn terminal_reply(event: &RioEvent, size: PtySize, screen: &TerminalScreen, palette: &List) -> Option<String> {
    match event {
        RioEvent::PtyWrite(_, reply) => Some(reply.clone()),
        RioEvent::TextAreaSizeRequest(_, format) => Some(format(WindowSize {
            rows: size.rows,
            cols: size.cols,
            width: size.pixel_width,
            height: size.pixel_height,
        })),
        RioEvent::ColorRequest(_, index, format) if *index < COUNT => {
            let color = screen.color_overrides()[*index].unwrap_or(palette[*index]);
            Some(format(ColorRgb::from_color_arr(color)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn terminal_tab_denies_clipboard_escape_sequences_but_preserves_bell() {
        let mut screen = TerminalScreen::new(8, 2, 10);
        let events = screen.process(b"\x1b]52;c;SGVsbG8=\x07\x1b]52;c;?\x07\x07");
        assert!(events.iter().any(|event| matches!(event, RioEvent::ClipboardStore(..))));
        assert!(events.iter().any(|event| matches!(event, RioEvent::ClipboardLoad(..))));
        assert!(
            events
                .iter()
                .filter(|event| matches!(event, RioEvent::ClipboardStore(..) | RioEvent::ClipboardLoad(..)))
                .all(|event| !may_forward_to_view(event))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RioEvent::Bell(..)) && may_forward_to_view(event))
        );
    }

    #[test]
    fn terminal_palette_tracks_theme_without_replacing_ansi_colors() {
        let mut colors = ThemeColor::default();
        colors.foreground = gpui_kit::rgb(0xf0e0d0).into();
        colors.background = gpui_kit::rgb(0x102030).into();
        colors.caret = gpui_kit::rgb(0x80a0c0).into();
        let palette = palette_for_theme(&colors);
        let expected = |color: Hsla| {
            let color: Rgba = color.into();
            [color.r, color.g, color.b, color.a]
        };
        assert_eq!(palette[NamedColor::Foreground], expected(colors.foreground));
        assert_eq!(palette[NamedColor::Background], expected(colors.background));
        assert_eq!(palette[NamedColor::Cursor], expected(colors.caret));
        assert_eq!(palette[NamedColor::Red], List::from(&Colors::default())[NamedColor::Red]);
    }

    #[test]
    fn terminal_tab_color_query_uses_palette_and_session_override() {
        let mut screen = TerminalScreen::new(8, 2, 10);
        let mut palette = List::from(&Colors::default());
        palette[1] = [0.0, 1.0, 0.0, 1.0];
        let size = PtySize::default();
        let events = screen.process(b"\x1b]4;1;?\x07");
        let response = events
            .into_iter()
            .find_map(|event| terminal_reply(&event, size, &screen, &palette))
            .unwrap();
        assert!(response.contains("rgb:0000/ffff/0000"));
        let _events = screen.process(b"\x1b]4;1;rgb:ff/00/00\x07");
        let events = screen.process(b"\x1b]4;1;?\x07");
        let response = events
            .into_iter()
            .find_map(|event| terminal_reply(&event, size, &screen, &palette))
            .unwrap();
        assert!(response.contains("rgb:ffff/0000/0000"));
    }

    #[test]
    fn terminal_tab_answers_pixel_size_queries_with_current_dimensions() {
        let mut screen = TerminalScreen::new(50, 5, 10);
        let first = PtySize {
            rows: 5,
            cols: 50,
            pixel_width: 640,
            pixel_height: 180,
        };
        let second = PtySize {
            rows: 9,
            cols: 70,
            pixel_width: 910,
            pixel_height: 270,
        };
        let replies = |events: Vec<RioEvent>, size| {
            events
                .into_iter()
                .filter_map(|event| {
                    terminal_reply(&event, size, &TerminalScreen::new(1, 1, 0), &List::from(&Colors::default()))
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(replies(screen.process(b"\x1b[14t"), first), ["\x1b[4;180;640t"]);
        let _events = screen.resize(second.cols, second.rows);
        assert_eq!(replies(screen.process(b"\x1b[14t"), second), ["\x1b[4;270;910t"]);
    }

    #[test]
    fn terminal_tab_drains_output_until_stream_closes_after_exit() {
        let dir = tempfile::tempdir().unwrap();
        let size = PtySize { rows: 4, cols: 40, pixel_width: 0, pixel_height: 0 };
        let mut tab =
            TerminalTab::spawn(dir.path(), Path::new("/bin/sh"), size, List::from(&Colors::default())).unwrap();
        tab.resize(PtySize { rows: 5, cols: 50, ..size }).unwrap();
        assert_eq!(tab.screen.grid().display_iter().count(), 250);
        tab.input(b"printf '\\007TAB_''MARKER'; exit 7\n".to_vec()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut bell_seen = false;
        let mut changed = false;
        while tab.exit_status.is_none() || !tab.output_closed {
            assert!(Instant::now() < deadline, "terminal did not finish");
            let drain = tab.drain();
            changed |= drain.changed;
            bell_seen |= drain.events.iter().any(|event| matches!(event, RioEvent::Bell(_)));
            std::thread::sleep(Duration::from_millis(10));
        }
        let text: String = tab.screen.grid().display_iter().map(|cell| cell.square.c()).collect();
        assert!(text.contains("TAB_MARKER"));
        assert!(bell_seen);
        assert!(changed);
        assert!(!tab.drain().changed);
        assert!(tab.exit_status.as_deref().unwrap().contains('7'));
        assert!(tab.error.is_none(), "{:?}", tab.error);
    }

    #[test]
    fn terminal_tab_tracks_application_cursor_mode() {
        let dir = tempfile::tempdir().unwrap();
        let size = PtySize { rows: 4, cols: 40, pixel_width: 0, pixel_height: 0 };
        let mut tab =
            TerminalTab::spawn(dir.path(), Path::new("/bin/sh"), size, List::from(&Colors::default())).unwrap();
        assert!(!tab.app_cursor());
        let _events = tab.screen.process(b"\x1b[?1h");
        assert!(tab.app_cursor());
        let _events = tab.screen.process(b"\x1b[?1l");
        assert!(!tab.app_cursor());
    }
}
