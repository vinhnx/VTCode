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
use rio_vt::crosswords::grid::Scroll;
use rio_vt::crosswords::pos::Side;
use rio_vt::event::{RioEvent, WindowSize};
use rio_vt::selection::SelectionType;

use super::terminal_process::{TerminalProcess, TerminalProcessEvent};
use super::terminal_screen::TerminalScreen;

const MAX_EVENTS_PER_DRAIN: usize = 64;
/// View-bound events kept per drain batch. Rio emits one event per BEL
/// with no coalescing, so a bell flood would otherwise grow this vec by
/// ~500k entries per pump tick; the pump drops these today and no
/// legitimate batch nears this ceiling.
const MAX_VIEW_EVENTS_PER_DRAIN: usize = 256;
const SCROLLBACK_LINES: usize = 10_000;

pub(super) struct TerminalDrain {
    pub(super) changed: bool,
    /// Raw view events for this drain. Production routes them through the
    /// pump's coalescer, so only tests read the vector directly.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "raw event list is consumed by the pump; tests inspect it")
    )]
    pub(super) events: Vec<RioEvent>,
}

/// Rio's compiled-in default table, parsed once: `Colors::default()` runs
/// `ColorBuilder::from_hex` per slot, and each parse compiles two regexes
/// with no cache, so rebuilding it per settings frame cost ~50ms (samply:
/// hundreds of NFA builds per frame across the five scheme previews). Rio
/// defaults never change within a binary; every palette below starts here
/// and overwrites its slots.
fn default_list() -> List {
    static DEFAULT: std::sync::LazyLock<List> = std::sync::LazyLock::new(|| List::from(&Colors::default()));
    *DEFAULT
}

pub(super) fn palette_for_theme(colors: &ThemeColor, dark: bool) -> List {
    let mut palette = default_list();
    if !dark {
        for (slot, color) in super::themes::light_ansi_slots().into_iter().enumerate() {
            palette[slot] = color;
        }
    }
    apply_chrome_slots(&mut palette, colors);
    palette
}

/// Fixed Ciapre colorscheme: the authored 16 slots regardless of mode,
/// with the default cells still tracking the active theme chrome.
pub(super) fn ciapre_palette(colors: &ThemeColor) -> List {
    let mut palette = default_list();
    for (slot, color) in super::themes::ciapre_ansi_slots().into_iter().enumerate() {
        palette[slot] = color;
    }
    apply_chrome_slots(&mut palette, colors);
    palette
}

/// Fixed Latte colorscheme: the authored 16 slots regardless of mode,
/// with the default cells still tracking the active theme chrome.
pub(super) fn latte_palette(colors: &ThemeColor) -> List {
    let mut palette = default_list();
    for (slot, color) in super::themes::latte_ansi_slots().into_iter().enumerate() {
        palette[slot] = color;
    }
    apply_chrome_slots(&mut palette, colors);
    palette
}

/// Default cells track the active theme in every palette: foreground,
/// background, and caret follow the chrome, never the ANSI table.
fn apply_chrome_slots(palette: &mut List, colors: &ThemeColor) {
    let rgba = |color: Hsla| {
        let color: Rgba = color.into();
        [color.r, color.g, color.b, color.a]
    };
    palette[NamedColor::Foreground] = rgba(colors.foreground);
    palette[NamedColor::Background] = rgba(colors.background);
    palette[NamedColor::Cursor] = rgba(colors.caret);
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
    #[cfg(test)]
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

    pub(super) fn in_alternate_screen(&self) -> bool {
        self.screen.in_alternate_screen()
    }

    pub(super) fn current_directory(&self) -> Option<std::path::PathBuf> {
        self.screen.current_directory().map(Path::to_path_buf)
    }

    pub(super) fn app_cursor(&self) -> bool {
        self.screen.mode().contains(Mode::APP_CURSOR)
    }

    pub(super) fn grid_size(&self) -> PtySize {
        self.size
    }

    pub(super) fn scroll_to(&mut self, target: Scroll) {
        let _events = self.screen.scroll_to(target);
    }

    pub(super) fn start_selection(&mut self, row: usize, column: usize, side: Side, ty: SelectionType) {
        self.screen.start_selection(row, column, side, ty);
    }

    pub(super) fn update_selection(&mut self, row: usize, column: usize, side: Side) {
        self.screen.update_selection(row, column, side);
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
                            if view_events.len() < MAX_VIEW_EVENTS_PER_DRAIN {
                                view_events.push(event);
                            }
                        }
                    }
                }
                Ok(TerminalProcessEvent::OutputClosed) => self.output_closed = true,
                Ok(TerminalProcessEvent::Exited(status)) => self.exit_status = Some(status),
                Ok(TerminalProcessEvent::Error(error)) => self.error = Some(error),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                    return TerminalDrain { changed: processed > 0, events: view_events };
                }
            }
        }
        TerminalDrain { changed: true, events: view_events }
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
    fn terminal_drain_routes_every_probe_exactly_once() {
        // (hostile bytes, witness that the probe parsed)
        type ProbeWitness = fn(&RioEvent) -> bool;
        let probes: &[(&[u8], ProbeWitness)] = &[
            (b"\x1b]52;c;SGVsbG8=\x07", |event| matches!(event, RioEvent::ClipboardStore(..))),
            (b"\x1b]52;c;?\x07", |event| matches!(event, RioEvent::ClipboardLoad(..))),
            (b"\x1b[6n", |event| matches!(event, RioEvent::PtyWrite(..))),
            (b"\x1b[14t", |event| matches!(event, RioEvent::TextAreaSizeRequest(..))),
            (b"\x1b]4;1;?\x07", |event| matches!(event, RioEvent::ColorRequest(..))),
            (b"\x07", |event| matches!(event, RioEvent::Bell(..))),
            (b"\x1b]2;owned-title\x07", |event| matches!(event, RioEvent::Title(..))),
            (b"\x1b]7;file:///etc\x07", |event| matches!(event, RioEvent::CurrentDirectoryChanged(..))),
        ];
        let palette = List::from(&Colors::default());
        for (bytes, witness) in probes {
            let mut screen = TerminalScreen::new(8, 2, 10);
            let events = screen.process(bytes);
            assert!(events.iter().any(witness), "probe parsed nothing: {bytes:?}");
            for event in &events {
                let replied = terminal_reply(event, PtySize::default(), &screen, &palette).is_some();
                let forwarded = may_forward_to_view(event);
                match event {
                    // Clipboard never touches the view or the PTY: dropped.
                    RioEvent::ClipboardStore(..) | RioEvent::ClipboardLoad(..) => {
                        assert!(!replied && !forwarded, "{event:?} must be dropped");
                    }
                    // Device/color/size queries answer to the PTY only.
                    RioEvent::PtyWrite(..) | RioEvent::TextAreaSizeRequest(..) | RioEvent::ColorRequest(..) => {
                        assert!(replied && !forwarded, "{event:?} must reply without forwarding");
                    }
                    // Bells, titles, cwd: view-bound only (the pump drops them today).
                    _ => assert!(!replied && forwarded, "{event:?} must forward without replying"),
                }
            }
        }
    }

    #[test]
    fn terminal_tab_drain_caps_view_events_under_bell_flood() {
        let dir = tempfile::tempdir().unwrap();
        let size = PtySize { rows: 4, cols: 40, pixel_width: 0, pixel_height: 0 };
        let mut tab =
            TerminalTab::spawn(dir.path(), Path::new("/bin/sh"), size, List::from(&Colors::default())).unwrap();
        // 16KB of BELs: Rio emits one event per bell with no coalescing,
        // so at least one drain batch overflows the cap 64x over.
        tab.input(b"awk 'BEGIN{for(i=0;i<16384;i++)printf \"\\a\"}'; exit 0\n".to_vec())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut max_events = 0;
        let mut bells_seen = 0;
        // Wait on the property itself, not a grid marker: any marker we type
        // also appears in the input echo, which broke the wait under load.
        loop {
            assert!(Instant::now() < deadline, "bell flood did not overflow the cap");
            let drain = tab.drain();
            max_events = max_events.max(drain.events.len());
            bells_seen += drain.events.iter().filter(|event| matches!(event, RioEvent::Bell(_))).count();
            if max_events == MAX_VIEW_EVENTS_PER_DRAIN && bells_seen > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(max_events, MAX_VIEW_EVENTS_PER_DRAIN, "no batch hit the cap: the flood never overflowed it");
        // Drain to stream end so teardown settles before the tab drops.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !tab.output_closed {
            assert!(Instant::now() < deadline, "stream did not close after the flood");
            let _drained = tab.drain();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn terminal_palette_tracks_theme_without_replacing_ansi_colors() {
        let mut colors = ThemeColor::default();
        colors.foreground = gpui_kit::rgb(0xf0e0d0).into();
        colors.background = gpui_kit::rgb(0x102030).into();
        colors.caret = gpui_kit::rgb(0x80a0c0).into();
        let palette = palette_for_theme(&colors, true);
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
    fn terminal_palette_dark_table_keeps_rio_defaults() {
        let colors = ThemeColor::default();
        let palette = palette_for_theme(&colors, true);
        let rio = List::from(&Colors::default());
        for slot in 0..16 {
            assert_eq!(palette[slot], rio[slot], "dark slot {slot} must stay Rio's default");
        }
    }

    #[test]
    fn terminal_palette_light_table_holds_contrast_on_every_light_theme() {
        use super::super::themes::{all_themes, contrast_ratio};
        let lights: Vec<_> = all_themes()
            .iter()
            .filter(|theme| theme.mode == gpui_kit::component::theme::ThemeMode::Light)
            .collect();
        assert!(!lights.is_empty(), "expected at least one built-in light theme");
        let colors = ThemeColor::default();
        let palette = palette_for_theme(&colors, false);
        let rio = List::from(&Colors::default());
        for slot in 0..16 {
            assert_ne!(palette[slot], rio[slot], "light slot {slot} must differ from dark");
            let [r, g, b, _] = palette[slot];
            let fg = Hsla::from(Rgba { r, g, b, a: 1.0 });
            for theme in &lights {
                let ratio = contrast_ratio(fg, theme.background);
                assert!(ratio >= 4.5, "{} slot {slot} fails AA: {ratio}", theme.id);
            }
        }
        // Theme chrome still wins for the default cells in both modes.
        let dark = palette_for_theme(&colors, true);
        assert_eq!(palette[NamedColor::Foreground], dark[NamedColor::Foreground]);
        assert_eq!(palette[NamedColor::Background], dark[NamedColor::Background]);
    }

    #[test]
    fn terminal_palette_ciapre_table_holds_contrast_on_ciapre_theme() {
        // Named-pairing proof, deliberately narrower than the light table's
        // every-theme sweep: a fixed muted scheme cannot hold 4.5 on
        // mid-dark grounds (zenburn #3F3F3F drags alert red to 4.0), and
        // chasing that would neon-ize the red past its own bright slot.
        // Rio's own dark table promises less (its black fails AA on every
        // dark ground), so the scheme promises AA on Ciapre, where darker
        // grounds only read better by monotonicity.
        use super::super::themes::{contrast_ratio, find};
        let ciapre = find("ciapre").expect("ciapre is the default theme");
        let colors = ThemeColor::default();
        let palette = ciapre_palette(&colors);
        let rio = List::from(&Colors::default());
        for slot in 0..16 {
            assert_ne!(palette[slot], rio[slot], "ciapre slot {slot} must differ from Rio dark");
            let [r, g, b, _] = palette[slot];
            let fg = Hsla::from(Rgba { r, g, b, a: 1.0 });
            let ratio = contrast_ratio(fg, ciapre.background);
            assert!(ratio >= 4.5, "ciapre slot {slot} fails AA: {ratio}");
        }
    }

    #[test]
    fn terminal_palette_ciapre_tracks_chrome_keeps_hue_true_slots() {
        let mut colors = ThemeColor::default();
        colors.foreground = gpui_kit::rgb(0xf0e0d0).into();
        colors.background = gpui_kit::rgb(0x102030).into();
        colors.caret = gpui_kit::rgb(0x80a0c0).into();
        let palette = ciapre_palette(&colors);
        let expected = |color: Hsla| {
            let color: Rgba = color.into();
            [color.r, color.g, color.b, color.a]
        };
        assert_eq!(palette[NamedColor::Foreground], expected(colors.foreground));
        assert_eq!(palette[NamedColor::Background], expected(colors.background));
        assert_eq!(palette[NamedColor::Cursor], expected(colors.caret));
        // The scheme slots ignore the active theme and stay hue-true: red
        // pins the authored alert-family hex, the rest prove dominance.
        let red = rio_vt::config::colors::ColorBuilder::from_hex(
            "#C16A68".to_owned(),
            rio_vt::config::colors::Format::SRGB0_1,
        )
        .map(|built| built.to_arr())
        .unwrap();
        assert_eq!(palette[NamedColor::Red], red);
        let [r, g, b, _] = palette[NamedColor::Red];
        assert!(r > g && r > b, "ciapre red must read red");
        let [r, g, b, _] = palette[NamedColor::Green];
        assert!(g > r && g > b, "ciapre green must read green");
        let [r, g, b, _] = palette[NamedColor::Yellow];
        assert!(r > b && g > b, "ciapre yellow must read yellow");
        let [r, g, b, _] = palette[NamedColor::Blue];
        assert!(b > r && b > g, "ciapre blue must read blue");
        let [r, g, b, _] = palette[NamedColor::Magenta];
        assert!(r > g && b > g, "ciapre magenta must read magenta");
        let [r, g, b, _] = palette[NamedColor::Cyan];
        assert!(g > r && b > r, "ciapre cyan must read cyan");
    }

    #[test]
    fn terminal_palette_latte_table_holds_contrast_on_latte_theme() {
        // Named-pairing proof like the Ciapre table: the fixed scheme
        // promises AA on its own latte ground, where darker grounds only
        // read better by monotonicity. The shared light table stays the
        // universal every-light option.
        use super::super::themes::{contrast_ratio, find};
        let latte = find("catppuccin-latte").expect("latte is the default light theme");
        let colors = ThemeColor::default();
        let palette = latte_palette(&colors);
        let rio = List::from(&Colors::default());
        for slot in 0..16 {
            assert_ne!(palette[slot], rio[slot], "latte slot {slot} must differ from Rio dark");
            let [r, g, b, _] = palette[slot];
            let fg = Hsla::from(Rgba { r, g, b, a: 1.0 });
            let ratio = contrast_ratio(fg, latte.background);
            assert!(ratio >= 4.5, "latte slot {slot} fails AA: {ratio}");
        }
    }

    #[test]
    fn terminal_palette_latte_tracks_chrome_keeps_hue_true_slots() {
        let mut colors = ThemeColor::default();
        colors.foreground = gpui_kit::rgb(0x202020).into();
        colors.background = gpui_kit::rgb(0xf0ece2).into();
        colors.caret = gpui_kit::rgb(0x406060).into();
        let palette = latte_palette(&colors);
        let expected = |color: Hsla| {
            let color: Rgba = color.into();
            [color.r, color.g, color.b, color.a]
        };
        assert_eq!(palette[NamedColor::Foreground], expected(colors.foreground));
        assert_eq!(palette[NamedColor::Background], expected(colors.background));
        assert_eq!(palette[NamedColor::Cursor], expected(colors.caret));
        // The scheme slots ignore the active theme and stay hue-true: red
        // pins the authored pink hex, the rest prove dominance.
        let red = rio_vt::config::colors::ColorBuilder::from_hex(
            "#C20E33".to_owned(),
            rio_vt::config::colors::Format::SRGB0_1,
        )
        .map(|built| built.to_arr())
        .unwrap();
        assert_eq!(palette[NamedColor::Red], red);
        let [r, g, b, _] = palette[NamedColor::Red];
        assert!(r > g && r > b, "latte red must read red");
        let [r, g, b, _] = palette[NamedColor::Green];
        assert!(g > r && g > b, "latte green must read green");
        let [r, g, b, _] = palette[NamedColor::Yellow];
        assert!(r > b && g > b, "latte yellow must read yellow");
        let [r, g, b, _] = palette[NamedColor::Blue];
        assert!(b > r && b > g, "latte blue must read blue");
        let [r, g, b, _] = palette[NamedColor::Magenta];
        assert!(r > g && b > g, "latte magenta must read magenta");
        let [r, g, b, _] = palette[NamedColor::Cyan];
        assert!(g > r && b > r, "latte cyan must read cyan");
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
