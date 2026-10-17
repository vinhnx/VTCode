#![cfg(unix)]
#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::super::*;
use super::helpers::*;

fn install_fake_clipboard(script_name: &str) -> (PathBuf, PathBuf, impl Drop) {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = std::env::temp_dir().join(format!(
        "vtcode-clipboard-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after UNIX_EPOCH")
            .as_nanos()
    ));
    fs::create_dir_all(&temp_dir).expect("create temp dir for clipboard script");
    struct TempDirGuard(PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let clipboard_file = temp_dir.join("clipboard.txt");
    fs::write(&clipboard_file, "").expect("create clipboard fixture");
    let script_path = temp_dir.join(script_name);
    fs::write(&script_path, format!("#!/bin/sh\ncat > '{}'\n", clipboard_file.display()))
        .expect("write fake clipboard command");
    let mut permissions = fs::metadata(&script_path).expect("read fake clipboard metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).expect("make fake clipboard executable");
    (temp_dir.clone(), clipboard_file, TempDirGuard(temp_dir))
}

/// Force both clipboard strategies to fail so `copy_on_select` behavior can be
/// asserted on selection state without touching the real system clipboard. The
/// native-helper probe is unspawnable and the OSC 52 fallback is stubbed out,
/// so these tests never race a detached helper or write user clipboard data.
struct FailingClipboardGuard;

impl FailingClipboardGuard {
    fn install() -> Self {
        use crate::tui::core_tui::session::mouse_selection::{
            set_clipboard_command_override, set_osc52_write_override,
        };

        set_clipboard_command_override(Some(PathBuf::from("/vtcode-test/missing-clipboard-helper")));
        set_osc52_write_override(Some(false));
        Self
    }
}

impl Drop for FailingClipboardGuard {
    fn drop(&mut self) {
        use crate::tui::core_tui::session::mouse_selection::{
            set_clipboard_command_override, set_osc52_write_override,
        };

        set_clipboard_command_override(None);
        set_osc52_write_override(None);
    }
}

#[test]
fn progress_only_transcript_never_copies_transient_text() {
    use crate::tui::core_tui::widgets::TranscriptWidget;
    use ratatui::{Terminal, backend::TestBackend};
    use vtcode_commons::ui_protocol::{ProgressOperation, ProgressPhase, ProgressUpdate};

    let _guard = CLIPBOARD_TEST_LOCK.lock().expect("clipboard test lock should not be poisoned");
    let _clipboard = FailingClipboardGuard::install();
    let mut session = Session::new(InlineTheme::default(), None, 12);
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation: ProgressOperation::start(),
        phase: ProgressPhase::WaitingForModel,
    }));
    session.mouse_selection.set_selection((0, 2), (20, 2));
    session.mouse_selection.request_copy();
    let mut terminal = Terminal::new(TestBackend::new(80, 4)).unwrap();
    terminal
        .draw(|frame| {
            TranscriptWidget::new(&mut session).render(Rect::new(0, 2, 80, 1), frame.buffer_mut());
            assert!(session.transcript_area().is_none());
            let viewport = frame.area();
            session.finalize_mouse_selection(frame, viewport);
        })
        .unwrap();
    assert!(session.copy_notification_text().is_none(), "no clipboard attempt may include progress text");
    assert!(!session.mouse_selection.needs_copy());
}

#[test]
fn manual_copy_mode_skips_input_auto_copy_but_ctrl_c_still_copies() {
    let _guard = CLIPBOARD_TEST_LOCK.lock().expect("clipboard test lock should not be poisoned");
    let _clipboard = FailingClipboardGuard::install();

    let mut session = app_session_with_input("hello world", "hello world".len());
    session.core.fullscreen.interaction.copy_on_select = false;
    for _ in 0..5 {
        let result = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        assert!(result.is_none());
    }

    assert_eq!(session.core.input_manager.selection_range(), Some(("hello world".len() - 5, "hello world".len())));

    // Rendering must not consume the pending selection while auto-copy is off.
    let _ = rendered_app_session_lines(&mut session, VIEW_ROWS);
    assert!(
        session.core.input_manager.selection_needs_copy(),
        "manual mode must keep the input selection pending until Ctrl+C"
    );

    // Only the explicit Ctrl+C request may copy the selection.
    let result = session.process_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(result.is_none());
    assert!(
        !session.core.input_manager.selection_needs_copy(),
        "Ctrl+C must copy the selection even in manual mode, and must not retry"
    );
}

#[test]
fn manual_copy_mode_skips_transcript_auto_copy_but_ctrl_c_still_copies() {
    let _guard = CLIPBOARD_TEST_LOCK.lock().expect("clipboard test lock should not be poisoned");
    let _clipboard = FailingClipboardGuard::install();

    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.fullscreen.interaction.copy_on_select = false;
    session.push_line(InlineMessageKind::Agent, vec![make_segment("hello world")]);

    let (transcript_area, rendered) = rendered_transcript_lines(&mut session, VIEW_ROWS * 2);
    let row = rendered
        .iter()
        .position(|line| line.contains("hello world"))
        .expect("expected hello world to be rendered");
    let column =
        rendered[row].find("hello").expect("expected hello word in rendered line") as u16 + transcript_area.x + 1;
    let row = transcript_area.y + row as u16;

    let (tx, _rx) = mpsc::unbounded_channel();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let release = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };

    session.handle_event(CrosstermEvent::Mouse(click), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(release), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(click), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(release), &tx, None);

    assert!(session.mouse_selection.has_selection);
    assert!(session.mouse_selection.needs_copy());

    // A render must not auto-copy while manual mode is enabled.
    let _ = rendered_transcript_lines(&mut session, VIEW_ROWS * 2);
    assert!(
        session.mouse_selection.needs_copy(),
        "manual mode must keep the transcript selection pending until Ctrl+C"
    );

    // An explicit copy request is flushed by the next frame's finalization.
    session.mouse_selection.request_copy();
    let _ = rendered_transcript_lines(&mut session, VIEW_ROWS * 2);
    assert!(
        !session.mouse_selection.needs_copy(),
        "an explicit copy request must be honored even in manual mode"
    );
}

#[test]
fn double_click_selects_transcript_word_and_copies_it() {
    use crate::tui::core_tui::session::mouse_selection::{clipboard_command_override, set_clipboard_command_override};
    use std::path::PathBuf;

    let _guard = CLIPBOARD_TEST_LOCK.lock().expect("clipboard test lock should not be poisoned");

    let script_name = if cfg!(target_os = "macos") { "pbcopy" } else { "xclip" };
    let (_temp_dir, clipboard_file, _temp_guard) = install_fake_clipboard(script_name);

    struct ClipboardCommandGuard(Option<PathBuf>);
    impl Drop for ClipboardCommandGuard {
        fn drop(&mut self) {
            set_clipboard_command_override(self.0.clone());
        }
    }

    let _path_guard = ClipboardCommandGuard(clipboard_command_override());
    set_clipboard_command_override(Some(clipboard_file.parent().expect("clipboard fixture parent").join(script_name)));

    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("hello world")]);

    let (transcript_area, rendered) = rendered_transcript_lines(&mut session, VIEW_ROWS * 2);
    let row = rendered
        .iter()
        .position(|line| line.contains("hello world"))
        .expect("expected hello world to be rendered");
    let column =
        rendered[row].find("hello").expect("expected hello word in rendered line") as u16 + transcript_area.x + 1;
    let row = transcript_area.y + row as u16;

    let (tx, _rx) = mpsc::unbounded_channel();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let release = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };

    session.handle_event(CrosstermEvent::Mouse(click), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(release), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(click), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(release), &tx, None);

    let mut buffer = Buffer::empty(Rect::new(0, 0, VIEW_WIDTH, VIEW_ROWS * 2));
    for (dy, line) in rendered.iter().enumerate() {
        for (dx, ch) in line.chars().enumerate() {
            buffer[(transcript_area.x + dx as u16, transcript_area.y + dy as u16)].set_symbol(&ch.to_string());
        }
    }
    let selected = session.mouse_selection.extract_text(&buffer, buffer.area);
    assert_eq!(selected, "hello");
    assert!(session.mouse_selection.has_selection);
    assert!(session.mouse_selection.needs_copy());

    session.copy_text_to_clipboard(&selected);
    session.mouse_selection.mark_copied();
    assert!(!session.mouse_selection.needs_copy());

    // Clipboard helpers are intentionally bounded and may finish just after
    // the copy call returns on a busy host. Give the detached helper a
    // generous grace period before asserting on its fixture output so parallel
    // test load cannot starve the writer thread.
    for _ in 0..500 {
        if fs::read_to_string(&clipboard_file).is_ok_and(|contents| contents == "hello") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let clipboard_contents = fs::read_to_string(&clipboard_file).expect("read copied transcript text");
    assert_eq!(clipboard_contents, "hello");

    let rendered_status = session
        .render_input_status_line(VIEW_WIDTH)
        .expect("input status line")
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(
        rendered_status.contains("copied 5 chars to clipboard"),
        "transcript copy should surface a temporary confirmation with char count, got: {rendered_status}"
    );
}

#[test]
fn selecting_input_text_auto_copies_and_keeps_selection() {
    use crate::tui::core_tui::session::mouse_selection::{clipboard_command_override, set_clipboard_command_override};
    use std::path::PathBuf;

    let _guard = CLIPBOARD_TEST_LOCK.lock().expect("clipboard test lock should not be poisoned");

    let script_name = if cfg!(target_os = "macos") { "pbcopy" } else { "xclip" };
    let (_temp_dir, clipboard_file, _temp_guard) = install_fake_clipboard(script_name);

    struct ClipboardCommandGuard(Option<PathBuf>);
    impl Drop for ClipboardCommandGuard {
        fn drop(&mut self) {
            set_clipboard_command_override(self.0.clone());
        }
    }

    let _path_guard = ClipboardCommandGuard(clipboard_command_override());
    set_clipboard_command_override(Some(clipboard_file.parent().expect("clipboard fixture parent").join(script_name)));

    let mut session = app_session_with_input("hello world", "hello world".len());
    for _ in 0..5 {
        let result = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        assert!(result.is_none());
    }

    assert_eq!(session.core.input_manager.selection_range(), Some(("hello world".len() - 5, "hello world".len())));

    let rendered = rendered_app_session_lines(&mut session, VIEW_ROWS);
    assert!(
        rendered.iter().any(|line| line.contains("copied 5 chars to clipboard")),
        "input copy should surface a temporary confirmation with char count"
    );

    for _ in 0..500 {
        if fs::read_to_string(&clipboard_file).is_ok_and(|contents| contents == "world") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let clipboard_contents = fs::read_to_string(&clipboard_file).expect("read copied input text");
    assert_eq!(clipboard_contents, "world");

    assert_eq!(session.core.input_manager.selection_range(), Some(("hello world".len() - 5, "hello world".len())));

    let result = session.process_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(result.is_none());
    assert_eq!(session.core.input_manager.selection_range(), Some(("hello world".len() - 5, "hello world".len())));

    let clipboard_contents = fs::read_to_string(&clipboard_file).expect("read copied input text");
    assert_eq!(clipboard_contents, "world");
}

#[test]
fn transcript_copy_failure_surfaces_copy_failed_status() {
    use crate::tui::core_tui::session::mouse_selection::{
        clipboard_command_override, set_clipboard_command_override, set_osc52_write_override,
    };
    use std::path::PathBuf;

    let _guard = CLIPBOARD_TEST_LOCK.lock().expect("clipboard test lock should not be poisoned");

    struct OverrideGuard;
    impl Drop for OverrideGuard {
        fn drop(&mut self) {
            set_clipboard_command_override(None);
            set_osc52_write_override(None);
        }
    }
    let _overrides = OverrideGuard;

    // An unspawnable helper fails before the bounded clipboard poll starts,
    // keeping this failure-path assertion deterministic under parallel load.
    set_clipboard_command_override(Some(PathBuf::from("/vtcode-test/missing-clipboard-helper")));
    set_osc52_write_override(Some(false));

    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.copy_text_to_clipboard("hello");

    let rendered_status = session
        .render_input_status_line(VIEW_WIDTH)
        .expect("input status line")
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(
        rendered_status.contains("Copy failed"),
        "failed copy should surface a failure notice, got: {rendered_status}"
    );
}

#[test]
fn input_copy_failure_is_swallowed_without_interrupt_and_never_retries() {
    use crate::tui::core_tui::session::mouse_selection::{
        clipboard_command_override, set_clipboard_command_override, set_osc52_write_override,
    };
    use std::path::PathBuf;

    let _guard = CLIPBOARD_TEST_LOCK.lock().expect("clipboard test lock should not be poisoned");

    struct OverrideGuard;
    impl Drop for OverrideGuard {
        fn drop(&mut self) {
            set_clipboard_command_override(None);
            set_osc52_write_override(None);
        }
    }
    let _overrides = OverrideGuard;

    // An unspawnable helper fails before the bounded clipboard poll starts,
    // keeping this failure-path assertion deterministic under parallel load.
    set_clipboard_command_override(Some(PathBuf::from("/vtcode-test/missing-clipboard-helper")));
    set_osc52_write_override(Some(false));

    let mut session = app_session_with_input("hello world", "hello world".len());
    for _ in 0..5 {
        let result = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        assert!(result.is_none());
    }

    let _ = rendered_app_session_lines(&mut session, VIEW_ROWS);
    assert!(
        !session.core.input_manager.selection_needs_copy(),
        "a failed auto-copy must not re-arm or every frame would retry it"
    );

    let result = session.process_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(result.is_none(), "Ctrl+C over a selection must stay a copy attempt even when copying fails");

    let status = session
        .core
        .render_input_status_line(VIEW_WIDTH)
        .expect("input status line after failed copy")
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(status.contains("Copy failed"), "failed copy should remain visible, got: {status}");
}
