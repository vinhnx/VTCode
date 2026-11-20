use super::text_utils::{char_display_width, display_width};
use ratatui::buffer::Buffer;
use ratatui::crossterm::{Command, clipboard::CopyToClipboard};
use ratatui::layout::Rect;
use std::io::Write;
#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::tui::config::constants::ui;
use crate::tui::core_tui::session::inline_list;

/// The modal list's selection cursor.
///
/// `┃` is the only glyph VT Code paints as a selection indicator, which makes
/// it unambiguous chrome. Transcript framing `│`/`├`/`└` prefixes are chrome
/// too and `extract_text` strips them on copy, but the transcript can still
/// hold `┃` as literal content (PTY output), so the cursor gutter is only
/// stripped on the overlay surface.
const LIST_SELECTION_CURSOR: &str = ui::MODAL_LIST_HIGHLIGHT_SYMBOL;

/// Light tree glyphs transcript chrome is framed with (`reflow/blocks.rs`).
/// Heavy variants (`┃`) are content or the modal cursor, never framing.
const TRANSCRIPT_FRAME_GLYPHS: [&str; 3] = ["│", "├", "└"];

const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(450);

/// Edge direction for auto-scroll while dragging a transcript selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DragAutoScrollDirection {
    Up,
    Down,
}

/// Pending edge auto-scroll for an in-progress transcript drag.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DragAutoScroll {
    pub(crate) direction: DragAutoScrollDirection,
    /// Pointer position clamped into the transcript area so the selection end
    /// keeps extending onto newly revealed rows.
    pub(crate) column: u16,
    pub(crate) row: u16,
    pub(crate) last_step: Instant,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SelectionSurface {
    #[default]
    Transcript,
    Overlay,
}

/// Tracks shared mouse-driven text selection for the transcript and overlays.
#[derive(Debug, Default)]
pub struct MouseSelectionState {
    /// Whether the user is currently dragging to select text.
    pub(crate) is_selecting: bool,
    /// Retained after mouse-up, unlike the active drag target.
    surface: SelectionSurface,
    /// Screen coordinates where the selection started (column, row).
    start: (u16, u16),
    /// Screen coordinates where the selection currently ends (column, row).
    end: (u16, u16),
    /// Whether a completed selection exists (ready for highlight rendering).
    pub(crate) has_selection: bool,
    /// Whether the current selection has already been copied to clipboard.
    copied: bool,
    /// Whether Ctrl+C was pressed to explicitly copy the current selection.
    copy_requested: bool,
    /// Tracks the previous mouse click so double-clicks can be detected.
    last_click: Option<ClickRecord>,
}

#[derive(Clone, Copy, Debug)]
struct ClickRecord {
    column: u16,
    row: u16,
    at: Instant,
}

impl MouseSelectionState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Begin a new selection at the given screen position.
    pub(crate) fn start_selection(&mut self, col: u16, row: u16) {
        self.surface = SelectionSurface::Transcript;
        self.is_selecting = true;
        self.has_selection = false;
        self.copied = false;
        self.start = (col, row);
        self.end = (col, row);
    }

    /// Begin an overlay selection that must not move with transcript geometry.
    pub(crate) fn start_overlay_selection(&mut self, col: u16, row: u16) {
        self.start_selection(col, row);
        self.surface = SelectionSurface::Overlay;
    }

    pub(crate) fn is_transcript_selection(&self) -> bool {
        self.surface == SelectionSurface::Transcript
    }

    /// Set a transcript selection directly, bypassing drag state.
    pub(crate) fn set_selection(&mut self, start: (u16, u16), end: (u16, u16)) {
        self.surface = SelectionSurface::Transcript;
        self.is_selecting = false;
        self.has_selection = start != end;
        self.copied = false;
        self.start = start;
        self.end = end;
    }

    /// Update the end position while dragging.
    pub(crate) fn update_selection(&mut self, col: u16, row: u16) {
        if self.is_selecting {
            self.end = (col, row);
            self.has_selection = true;
        }
    }

    /// Finalize the selection on mouse-up.
    pub(crate) fn finish_selection(&mut self, col: u16, row: u16) {
        if self.is_selecting {
            self.end = (col, row);
            self.is_selecting = false;
            // Only mark as having a selection if start != end
            self.has_selection = self.start != self.end;
        }
    }

    /// Adjust transcript selection coordinates when the underlying content moves.
    /// Overlay text retains its screen position during background scrolling.
    ///
    /// `row_delta` is positive when content moves down on screen (scroll up / showing
    /// older content) and negative when content moves up (scroll down / showing newer
    /// content).  If the adjustment pushes the selection completely off-screen the
    /// selection is cleared.
    pub(crate) fn adjust_for_scroll(&mut self, row_delta: i32) {
        if !self.is_transcript_selection() || (!self.has_selection && !self.is_selecting) {
            return;
        }
        if row_delta == 0 {
            return;
        }

        let new_start_row = self.start.1 as i32 + row_delta;
        let new_end_row = self.end.1 as i32 + row_delta;

        // If both ends are completely off-screen in the same direction, clear.
        // Clamp to screen bounds (0..=viewport_height, roughly 0..=u16::MAX).
        // If after clamping both are the same and off-screen, or both are off-screen
        // in a way that suggests selection is gone, clear.

        let clamped_start = new_start_row.clamp(0, i32::from(u16::MAX));
        let clamped_end = new_end_row.clamp(0, i32::from(u16::MAX));

        // If the selection is now completely off-screen in a way that means
        // the original selection range was entirely off-screen, clear.
        if (new_start_row < 0 && new_end_row < 0)
            || (new_start_row > i32::from(u16::MAX) && new_end_row > i32::from(u16::MAX))
        {
            self.is_selecting = false;
            self.has_selection = false;
            self.copied = false;
            self.copy_requested = false;
            return;
        }

        self.start.1 = clamped_start as u16;
        self.end.1 = clamped_end as u16;
    }

    /// Clear any active selection.
    pub(crate) fn clear(&mut self) {
        self.surface = SelectionSurface::Transcript;
        self.is_selecting = false;
        self.has_selection = false;
        self.copied = false;
        self.copy_requested = false;
        self.last_click = None;
    }

    /// Clears only the mouse click history used for double-click detection.
    pub(crate) fn clear_click_history(&mut self) {
        self.last_click = None;
    }

    /// Records a click and returns `true` when it matches the previous click closely enough
    /// to be treated as a double click.
    pub(crate) fn register_click(&mut self, col: u16, row: u16, at: Instant) -> bool {
        let is_double_click = self.last_click.is_some_and(|last| {
            last.column == col && last.row == row && at.saturating_duration_since(last.at) <= DOUBLE_CLICK_INTERVAL
        });

        self.last_click = Some(ClickRecord { column: col, row, at });
        is_double_click
    }

    /// Returns the selection range normalized so that `from` is before `to`.
    fn normalized(&self) -> ((u16, u16), (u16, u16)) {
        let (s, e) = (self.start, self.end);
        if s.1 < e.1 || (s.1 == e.1 && s.0 <= e.0) {
            (s, e)
        } else {
            (e, s)
        }
    }

    /// Extract selected text from a ratatui `Buffer`.
    pub(crate) fn extract_text(&self, buf: &Buffer, area: Rect) -> String {
        if !self.has_selection && !self.is_selecting {
            return String::new();
        }

        // Clamp to the actual buffer area to avoid out-of-range buffer indexing panics.
        let area = area.intersection(buf.area);
        if area.width == 0 || area.height == 0 {
            return String::new();
        }

        let ((start_col, start_row), (end_col, end_row)) = self.normalized();

        // The selection cursor is chrome, not content: the gutter it sits in
        // marks the focused list row, and a copy that carried the marker
        // would paste something that means nothing outside the TUI. Only the
        // overlay surface draws that gutter; a transcript row may hold the
        // glyph as ordinary content.
        let strip_gutter = self.surface == SelectionSurface::Overlay;

        // Collect the rows as (screen column, symbol) pairs.
        let mut rows: Vec<Vec<(u16, &str)>> = Vec::new();
        let mut cursor_columns: Vec<u16> = Vec::new();

        for row in start_row..=end_row {
            if row < area.y || row >= area.bottom() {
                continue;
            }
            let line_start = if row == start_row {
                start_col.max(area.x)
            } else {
                area.x
            };
            let line_end = if row == end_row {
                end_col.min(area.right())
            } else {
                area.right()
            };

            let mut cells: Vec<(u16, &str)> = Vec::new();
            for col in line_start..line_end {
                if col < area.x || col >= area.right() {
                    continue;
                }
                let symbol = buf[(col, row)].symbol();
                if symbol.is_empty() {
                    continue;
                }
                if strip_gutter && symbol == LIST_SELECTION_CURSOR && !cursor_columns.contains(&col) {
                    cursor_columns.push(col);
                }
                cells.push((col, symbol));
            }
            rows.push(cells);
        }

        // Transcript chrome — block framing (`  │ `, `└ `, `├ `) and
        // markdown-table rules (`───`) — is decoration drawn from box-drawing
        // glyphs. A copy that carries it pastes the "│ cleanup dance" into
        // editors, so strip it from transcript copies; overlay surfaces keep
        // their cells verbatim (the tool-output viewer renders raw, unframed
        // lines already).
        let rows = if self.surface == SelectionSurface::Transcript {
            rows.into_iter().filter_map(strip_transcript_chrome).collect::<Vec<_>>()
        } else {
            rows
        };

        // The list gutter is the marker plus its trailing padding
        // (`MODAL_LIST_HIGHLIGHT_FULL`). Drop that whole span wherever it
        // falls inside the selection, so the copy neither carries the marker
        // nor keeps its orphaned indentation. A cell holding real content
        // stays: the gutter is only ever blank or the marker itself.
        let gutter_width = u16::try_from(inline_list::selection_padding_width()).unwrap_or(1);
        let is_gutter_cell = |col: u16, symbol: &str| {
            strip_gutter
                && cursor_columns
                    .iter()
                    .any(|start| col >= *start && col < start.saturating_add(gutter_width))
                && (symbol.trim().is_empty() || symbol == LIST_SELECTION_CURSOR)
        };

        let mut result = String::new();
        for (index, cells) in rows.iter().enumerate() {
            for (col, symbol) in cells {
                if is_gutter_cell(*col, symbol) {
                    continue;
                }
                result.push_str(symbol);
            }

            // Add newline between rows (but not after the last)
            if index < rows.len() - 1 {
                // Trim trailing whitespace from each line
                let trimmed = result.trim_end().len();
                result.truncate(trimmed);
                result.push('\n');
            }
        }

        // Trim trailing whitespace from the final line
        let trimmed = result.trim_end();
        trimmed.to_string()
    }

    /// Apply selection highlight (inverted colors) to the frame buffer.
    pub(crate) fn apply_highlight(&self, buf: &mut Buffer, area: Rect) {
        if !self.has_selection && !self.is_selecting {
            return;
        }

        // Clamp to the actual buffer area to avoid out-of-range buffer indexing panics.
        let area = area.intersection(buf.area);
        if area.width == 0 || area.height == 0 {
            return;
        }

        let ((start_col, start_row), (end_col, end_row)) = self.normalized();

        for row in start_row..=end_row {
            if row < area.y || row >= area.bottom() {
                continue;
            }
            let line_start = if row == start_row {
                start_col.max(area.x)
            } else {
                area.x
            };
            let line_end = if row == end_row {
                end_col.min(area.right())
            } else {
                area.right()
            };

            for col in line_start..line_end {
                if col < area.x || col >= area.right() {
                    continue;
                }
                let cell = &mut buf[(col, row)];
                // Swap foreground and background to show selection
                let fg = cell.fg;
                let bg = cell.bg;
                cell.set_fg(bg);
                cell.set_bg(fg);
            }
        }
    }

    /// Returns `true` if the selection needs to be copied (finalized and not yet copied).
    pub(crate) fn needs_copy(&self) -> bool {
        self.has_selection && !self.is_selecting && !self.copied
    }

    /// Returns `true` if an explicit copy was requested via Ctrl+C.
    pub(crate) fn has_copy_request(&self) -> bool {
        self.copy_requested
    }

    /// Request an explicit copy of the current selection (triggered by Ctrl+C).
    pub(crate) fn request_copy(&mut self) {
        if self.has_selection {
            self.copy_requested = true;
        }
    }

    /// Mark the selection as already copied.
    pub(crate) fn mark_copied(&mut self) {
        self.copied = true;
    }

    /// Copy the selected text to the system clipboard.
    ///
    /// Tries native OS clipboard utilities first (`pbcopy` on macOS, `xclip`/`xsel`/`wl-copy`
    /// on Linux, `clip.exe` on Windows/WSL) for maximum compatibility, then falls back to the
    /// OSC 52 escape sequence. Returns whether any strategy reported success.
    ///
    /// OSC 52 delivery is best-effort: terminals never acknowledge the sequence, so a clean
    /// stderr write counts as success even if the terminal silently drops it (unsupported,
    /// disabled, or swallowed by a multiplexer). Every strategy attempt emits a
    /// `tracing::debug!` event; enable `RUST_LOG=vtcode_ui=debug` when diagnosing.
    pub fn copy_to_clipboard(text: &str) -> bool {
        if text.is_empty() {
            trace_clipboard("nothing to copy: empty text");
            return false;
        }

        if Self::copy_via_native(text) {
            return true;
        }

        let copied = copy_via_osc52(text, inside_tmux());
        trace_clipboard(&format!(
            "native candidates exhausted; osc52 fallback result={copied} text_bytes={}",
            text.len()
        ));
        copied
    }

    /// Attempt to copy text using native OS clipboard utilities.
    /// Returns `true` if successful.
    fn copy_via_native(text: &str) -> bool {
        use std::process::Command;

        #[cfg(test)]
        if let Some(program) = clipboard_command_override() {
            return spawn_clipboard_command(Command::new(program), text);
        }

        let candidates: &[&str] = if cfg!(target_os = "macos") {
            &["pbcopy"]
        } else if cfg!(target_os = "linux") {
            &["xclip", "xsel", "wl-copy"]
        } else if cfg!(target_os = "windows") {
            &["clip.exe"]
        } else {
            &[]
        };

        for program in candidates {
            let mut cmd = Command::new(program);
            match *program {
                "xclip" => {
                    cmd.arg("-selection").arg("clipboard");
                }
                "xsel" => {
                    cmd.arg("--clipboard").arg("--input");
                }
                _ => {}
            }
            if spawn_clipboard_command(cmd, text) {
                trace_clipboard(&format!("native '{program}' succeeded ({} bytes)", text.len()));
                return true;
            }
        }
        trace_clipboard("all native clipboard candidates failed or missing");
        false
    }
}

/// Emits clipboard strategy diagnostics for support/debugging sessions.
fn trace_clipboard(message: &str) {
    tracing::debug!(target: "vtcode_ui::clipboard", "{message}");
}

/// Waits on `child` from a detached thread so no zombie accumulates and the caller never blocks.
fn reap_in_background(mut child: std::process::Child) {
    let _ = std::thread::spawn(move || {
        let _ = child.wait();
    });
}

fn spawn_clipboard_command(mut cmd: std::process::Command, text: &str) -> bool {
    use std::process::Stdio;

    // Helpers such as xclip/wl-copy normally exit right away, but some builds
    // fork-and-hold while serving the selection, and a helper that never reads
    // stdin could block a synchronous write_all forever once the pipe buffer
    // fills (>= 64KB). The input is therefore written on a detached writer
    // thread and the result is read via try_recv, so the UI thread never blocks
    // longer than this budget regardless of helper behavior.
    const WAIT_BUDGET: Duration = Duration::from_millis(300);
    const POLL_INTERVAL: Duration = Duration::from_millis(10);
    // After the helper exits, its writer report is usually already delivered;
    // this grace window only covers the rare in-flight race.
    const WRITE_REPORT_GRACE: Duration = Duration::from_millis(100);

    let program = cmd.get_program().to_string_lossy().into_owned();
    let Ok(mut child) = cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        trace_clipboard(&format!("'{program}' unavailable: spawn failed"));
        return false;
    };
    // The writer is detached and never joined so a blocked write_all cannot
    // stall the UI thread; only the stdin pipe (plus a copy of the text) moves
    // into it. `child` stays on this thread for try_wait polling and is handed
    // to a detached reaper if the budget expires.
    let mut stdin = child.stdin.take();
    let text_owned = text.to_owned();
    let (write_tx, write_rx) = std::sync::mpsc::channel();
    let _ = std::thread::spawn(move || {
        let result = match stdin.as_mut() {
            Some(pipe) => pipe.write_all(text_owned.as_bytes()).err(),
            None => None,
        };
        drop(stdin.take());
        let _ = write_tx.send(result);
    });
    // Tracks the first observed write failure; None until the writer reports or
    // a status/deadline path makes a decision.
    let mut write_error = None;
    let mut write_complete = false;
    let deadline = Instant::now() + WAIT_BUDGET;
    loop {
        if let Ok(report) = write_rx.try_recv() {
            write_error = report;
            write_complete = true;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // The helper has exited, so the writer will finish promptly;
                // give its report a short grace window, otherwise a write
                // failing concurrently with a clean exit would count as ok.
                let flush_deadline = Instant::now() + WRITE_REPORT_GRACE;
                while !write_complete {
                    match write_rx.try_recv() {
                        Ok(report) => {
                            write_error = report;
                            write_complete = true;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            if Instant::now() >= flush_deadline {
                                break;
                            }
                            std::thread::sleep(POLL_INTERVAL);
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                }
                let ok = status.success() && write_error.is_none();
                if !ok {
                    if let Some(err) = write_error {
                        trace_clipboard(&format!("'{program}' stdin write failed: {err}; status={status}"));
                    } else {
                        trace_clipboard(&format!("'{program}' failed: status={status}"));
                    }
                }
                return ok;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    // The writer may still be blocked; do NOT join it (that
                    // would hang the UI thread). Input was at least accepted
                    // into the pipe unless a write failure was already reported.
                    let ok = write_error.is_none();
                    trace_clipboard(&format!(
                        "'{program}' still running after {WAIT_BUDGET:?}; input {}",
                        if ok { "accepted" } else { "failed" }
                    ));
                    // Fork-and-hold helpers legitimately outlive the copy call.
                    reap_in_background(child);
                    return ok;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(err) => {
                trace_clipboard(&format!("'{program}' wait failed: {err}"));
                reap_in_background(child);
                return false;
            }
        }
    }
}

/// Whether the session runs inside tmux; OSC 52 must then be DCS-wrapped or tmux
/// consumes it without ever reaching the outer terminal.
fn inside_tmux() -> bool {
    std::env::var_os("TMUX").is_some_and(|value| !value.is_empty())
}

/// Wrap an escape sequence in tmux's DCS passthrough so tmux forwards it verbatim to
/// the outer terminal. Every ESC inside the payload must be doubled.
fn wrap_tmux_passthrough(sequence: &str) -> String {
    format!("\x1bPtmux;\x1b{}\x1b\\", sequence.replace('\x1b', "\x1b\x1b"))
}

/// Build the OSC 52 payload for `text`, wrapped for tmux when requested.
fn build_osc52_payload(text: &str, inside_tmux: bool) -> Option<String> {
    let mut sequence = String::new();
    CopyToClipboard::to_clipboard_from(text.as_bytes())
        .write_ansi(&mut sequence)
        .ok()?;
    Some(if inside_tmux {
        wrap_tmux_passthrough(&sequence)
    } else {
        sequence
    })
}

/// Emit the OSC 52 clipboard sequence on stderr. Delivery cannot be confirmed —
/// terminals never acknowledge it — so success means "written without IO error".
fn copy_via_osc52(text: &str, inside_tmux: bool) -> bool {
    #[cfg(test)]
    if let Some(forced) = osc52_write_override() {
        return forced;
    }

    let Some(payload) = build_osc52_payload(text, inside_tmux) else {
        return false;
    };

    let mut stderr = std::io::stderr();
    let written = stderr.write_all(payload.as_bytes()).is_ok() && stderr.flush().is_ok();
    trace_clipboard(&format!("osc52 written={written} payload_bytes={} tmux_passthrough={inside_tmux}", payload.len()));
    written
}

#[cfg(test)]
static CLIPBOARD_COMMAND_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

#[cfg(test)]
static OSC52_WRITE_OVERRIDE: OnceLock<Mutex<Option<bool>>> = OnceLock::new();

#[cfg(test)]
pub(crate) fn set_osc52_write_override(result: Option<bool>) {
    let lock = OSC52_WRITE_OVERRIDE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = lock.lock() {
        *guard = result;
    }
}

#[cfg(test)]
fn osc52_write_override() -> Option<bool> {
    let lock = OSC52_WRITE_OVERRIDE.get_or_init(|| Mutex::new(None));
    lock.lock().ok().and_then(|guard| *guard)
}

#[cfg(test)]
pub(crate) fn set_clipboard_command_override(path: Option<PathBuf>) {
    let lock = CLIPBOARD_COMMAND_OVERRIDE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = lock.lock() {
        *guard = path;
    }
}

#[cfg(test)]
pub(crate) fn clipboard_command_override() -> Option<PathBuf> {
    let lock = CLIPBOARD_COMMAND_OVERRIDE.get_or_init(|| Mutex::new(None));
    match lock.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => None,
    }
}

/// One copied transcript row after chrome removal, or `None` when the row is
/// pure chrome and disappears from the copy.
///
/// Chrome forms sit at zero or two leading spaces: tree rows start at the
/// row edge and PTY command-header continuations use the exact `  │ ` prefix.
/// PTY program output is indented four spaces, so content that starts with
/// the same glyphs keeps them; `tree` output like `├── src` survives because
/// the glyph is followed by `─`, not the prefix's separating space.
fn strip_transcript_chrome(cells: Vec<(u16, &str)>) -> Option<Vec<(u16, &str)>> {
    let is_box_drawing = |symbol: &str| {
        let mut chars = symbol.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => ('\u{2500}'..='\u{257F}').contains(&c),
            _ => false,
        }
    };
    let leading_spaces = cells.iter().take_while(|(_, symbol)| *symbol == " ").count();
    let chrome_indented = leading_spaces <= 2;

    // A rule row (markdown-table separator): box-drawing glyphs plus padding
    // only, with at least one glyph.
    let has_glyph = cells.iter().any(|(_, symbol)| is_box_drawing(symbol));
    if chrome_indented
        && has_glyph
        && cells
            .iter()
            .all(|(_, symbol)| is_box_drawing(symbol) || symbol.trim().is_empty())
    {
        return None;
    }

    // A framing prefix: the glyph plus its separating space; the indent stays.
    if chrome_indented {
        if let Some((_, glyph)) = cells.get(leading_spaces) {
            let following_space = cells.get(leading_spaces + 1).is_some_and(|(_, symbol)| *symbol == " ");
            if TRANSCRIPT_FRAME_GLYPHS.contains(glyph) && following_space {
                let mut kept: Vec<(u16, &str)> = cells[..leading_spaces].to_vec();
                kept.extend_from_slice(&cells[leading_spaces + 2..]);
                return Some(kept);
            }
        }
    }
    Some(cells)
}

/// Return the half-open display-column range for the word under `column`.
pub(crate) fn word_selection_range(text: &str, column: u16) -> Option<(u16, u16)> {
    if text.is_empty() {
        return None;
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let line_width = display_width(text);
    if usize::from(column) >= line_width {
        return None;
    }

    let mut consumed = 0usize;
    let mut char_index = 0usize;
    for ch in &chars {
        let width = char_display_width(*ch);
        if consumed.saturating_add(width) > usize::from(column) {
            break;
        }
        consumed = consumed.saturating_add(width);
        char_index += 1;
    }

    if char_index >= chars.len() || chars[char_index].is_whitespace() {
        return None;
    }

    let mut start = char_index;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }

    let mut end = char_index + 1;
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }

    Some((display_width_for_char_count(&chars, start), display_width_for_char_count(&chars, end)))
}

fn display_width_for_char_count(chars: &[char], char_count: usize) -> u16 {
    chars
        .iter()
        .take(char_count)
        .map(|&ch| char_display_width(ch) as u16)
        .fold(0_u16, u16::saturating_add)
}

#[cfg(test)]
mod tests;
