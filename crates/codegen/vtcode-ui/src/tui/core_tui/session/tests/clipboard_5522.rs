#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use std::sync::MutexGuard;
use std::time::Instant;

use super::helpers::*;
use crate::tui::core_tui::session::clipboard_5522::{set_clipboard_5522_capture, take_clipboard_5522_captured};
use crate::tui::core_tui::session::clipboard_image::ClipboardImageError;
use crate::tui::core_tui::types::ContentPart;
use vtcode_commons::clipboard_5522::set_clipboard_5522_hint;
use vtcode_commons::env_lock::{EnvGuard, lock as lock_env};

const CLIENT_NAME_B64: &str = "VlQgQ29kZQ=="; // "VT Code"
const TEXT_PLAIN_B64: &str = "dGV4dC9wbGFpbg=="; // "text/plain"
const IMAGE_PNG_B64: &str = "aW1hZ2UvcG5n"; // "image/png"
const LIST_WITH_IMAGE_B64: &str = "dGV4dC9wbGFpbiBpbWFnZS9wbmc="; // "text/plain image/png"
const PNG_MAGIC_B64: &str = "iVBORw0KGgo="; // PNG signature bytes

/// Serializes the 5522 globals (probe hint, query capture, env override) and
/// restores a clean slate on drop, so fallback tests never leak support into
/// the synchronous-warning tests.
struct Clipboard5522Guard {
    _clipboard: MutexGuard<'static, ()>,
    env: EnvGuard,
    prev_env: Option<std::ffi::OsString>,
}

impl Clipboard5522Guard {
    fn with_support(supported: bool) -> Self {
        let clipboard = CLIPBOARD_TEST_LOCK.lock().expect("clipboard test lock should not be poisoned");
        let env = lock_env();
        let prev_env = std::env::var_os("VTCODE_CLIPBOARD_5522");
        env.remove_var("VTCODE_CLIPBOARD_5522");
        set_clipboard_5522_hint(Some(supported));
        set_clipboard_5522_capture(true);
        Self { _clipboard: clipboard, env, prev_env }
    }
}

impl Drop for Clipboard5522Guard {
    fn drop(&mut self) {
        set_clipboard_5522_capture(false);
        let _ = take_clipboard_5522_captured();
        set_clipboard_5522_hint(None);
        self.env.restore_var("VTCODE_CLIPBOARD_5522", self.prev_env.take());
    }
}

fn image_paste_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)
}

fn enable_image_input(session: &mut AppSession) {
    session.handle_command(app_types::InlineCommand::SetImageInputEnabled(true));
}

fn warning_text(session: &AppSession) -> String {
    session
        .core
        .lines
        .iter()
        .filter(|line| line.kind == InlineMessageKind::Warning)
        .flat_map(|line| line.segments.iter())
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join("")
}

/// Press Ctrl+V with a failing OS reader: starts the 5522 fallback when
/// supported, warns synchronously otherwise.
fn press_paste_with_failing_os_reader(session: &mut AppSession) {
    enable_image_input(session);
    let event = session
        .process_key_with_clipboard_image_reader(image_paste_key(), || Err(ClipboardImageError::ClipboardUnavailable));
    assert!(event.is_none());
}

fn feed_packet(session: &mut AppSession, packet: &str) -> bool {
    session.core.handle_clipboard_5522_packet(packet)
}

fn feed_list_reply(session: &mut AppSession, list_b64: &str) {
    assert!(!feed_packet(session, "5522;type=read:status=OK"));
    assert!(!feed_packet(session, &format!("5522;type=read:status=DATA:mime={TEXT_PLAIN_B64};{list_b64}")));
    assert!(!feed_packet(session, "5522;type=read:status=DONE"));
}

fn request_id(query: &str) -> &str {
    let start = query.find(":id=").expect("query carries a request id") + 4;
    let rest = &query[start..];
    let end = rest.find([':', ';']).expect("request id terminator");
    &rest[..end]
}

#[test]
fn fallback_sends_mime_list_query_when_os_read_fails() {
    let _guard = Clipboard5522Guard::with_support(true);
    let mut session = app_session_with_input("describe this", "describe this".len());
    assert!(!session.core.needs_animation_tick());

    press_paste_with_failing_os_reader(&mut session);

    assert!(warning_text(&session).is_empty(), "no synchronous warning while the async read is pending");
    assert!(session.core.needs_animation_tick(), "pending read arms a transient tick deadline");
    let queries = take_clipboard_5522_captured();
    assert_eq!(queries.len(), 1);
    let query = &queries[0];
    assert!(query.starts_with("\x1b]5522;type=read"), "{query:?}");
    assert!(query.contains(":pw=") && query.contains(&format!(":name={CLIENT_NAME_B64}")), "{query:?}");
    assert!(query.ends_with(";Lg==\x1b\\"), "{query:?}");
    assert!(!request_id(query).is_empty());
}

#[test]
fn mime_list_without_image_warns_no_image() {
    let _guard = Clipboard5522Guard::with_support(true);
    let mut session = app_session_with_input("", 0);
    press_paste_with_failing_os_reader(&mut session);

    feed_list_reply(&mut session, TEXT_PLAIN_B64);

    assert_eq!(warning_text(&session), "No image found in clipboard.");
    assert_eq!(take_clipboard_5522_captured().len(), 1, "no typed read without an image MIME type");
    // Pending state is cleared: stray packets after completion are ignored.
    assert!(!feed_packet(&mut session, "5522;type=read:status=DONE"));
    assert_eq!(warning_text(&session), "No image found in clipboard.");
}

#[test]
fn mime_list_with_image_reads_and_attaches() {
    let _guard = Clipboard5522Guard::with_support(true);
    let mut session = app_session_with_input("describe this", "describe this".len());
    press_paste_with_failing_os_reader(&mut session);

    feed_list_reply(&mut session, LIST_WITH_IMAGE_B64);

    let queries = take_clipboard_5522_captured();
    assert_eq!(queries.len(), 2);
    assert!(queries[1].contains(&format!(";{IMAGE_PNG_B64}\x1b\\")), "typed read: {:?}", queries[1]);

    assert!(!feed_packet(&mut session, "5522;type=read:status=OK"));
    assert!(!feed_packet(
        &mut session,
        &format!("5522;type=read:status=DATA:mime={IMAGE_PNG_B64};{PNG_MAGIC_B64}")
    ));
    assert!(feed_packet(&mut session, "5522;type=read:status=DONE"));

    assert_eq!(session.core.input_manager.content(), "describe this[Image #1]");
    assert_eq!(session.core.input_manager.attachments(), &[ContentPart::image(PNG_MAGIC_B64, "image/png")]);
    assert!(warning_text(&session).is_empty());
}

#[test]
fn terminal_error_warns_unavailable_and_clears_pending() {
    let _guard = Clipboard5522Guard::with_support(true);
    let mut session = app_session_with_input("", 0);
    press_paste_with_failing_os_reader(&mut session);

    assert!(!feed_packet(&mut session, "5522;type=read:status=OK"));
    assert!(!feed_packet(&mut session, "5522;type=read:status=EPERM"));

    assert_eq!(warning_text(&session), "Clipboard image paste is unavailable in this terminal or desktop session.");
    assert!(!feed_packet(&mut session, "5522;type=read:status=DONE"));
}

#[test]
fn foreign_id_replies_are_ignored_while_pending() {
    let _guard = Clipboard5522Guard::with_support(true);
    let mut session = app_session_with_input("", 0);
    press_paste_with_failing_os_reader(&mut session);
    let real_id = request_id(&take_clipboard_5522_captured()[0]).to_string();

    assert!(!feed_packet(&mut session, "5522;type=read:status=OK:id=not-ours"));
    assert!(warning_text(&session).is_empty(), "foreign replies must not warn or abort");

    // Our own reply still completes the pending read.
    assert!(!feed_packet(&mut session, &format!("5522;type=read:status=OK:id={real_id}")));
    assert!(!feed_packet(
        &mut session,
        &format!("5522;type=read:status=DATA:mime={TEXT_PLAIN_B64};{LIST_WITH_IMAGE_B64}")
    ));
    assert!(!feed_packet(&mut session, &format!("5522;type=read:status=DONE:id={real_id}")));
    // The list query was drained above to read its id; only the typed read remains.
    let queries = take_clipboard_5522_captured();
    assert_eq!(queries.len(), 1);
    assert!(queries[0].contains(&format!(";{IMAGE_PNG_B64}\x1b\\")), "typed read: {:?}", queries[0]);
}

#[test]
fn malformed_packet_aborts_with_warning() {
    let _guard = Clipboard5522Guard::with_support(true);
    let mut session = app_session_with_input("", 0);
    press_paste_with_failing_os_reader(&mut session);

    assert!(!feed_packet(&mut session, "not-a-5522-packet"));

    assert_eq!(warning_text(&session), "Clipboard image paste is unavailable in this terminal or desktop session.");
}

#[test]
fn expired_read_warns_on_tick() {
    let _guard = Clipboard5522Guard::with_support(true);
    let mut session = app_session_with_input("", 0);
    press_paste_with_failing_os_reader(&mut session);

    session.core.handle_tick();
    assert!(warning_text(&session).is_empty(), "fresh reads must survive routine ticks");

    let past = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("past deadline should exist");
    session.core.set_clipboard_5522_deadline_for_tests(past);
    session.core.handle_tick();
    assert_eq!(warning_text(&session), "Clipboard image paste is unavailable in this terminal or desktop session.");
    assert!(!feed_packet(&mut session, "5522;type=read:status=OK"), "late replies stay ignored");
}

#[test]
fn no_fallback_without_support_warns_synchronously() {
    let _guard = Clipboard5522Guard::with_support(false);
    let mut session = app_session_with_input("", 0);
    press_paste_with_failing_os_reader(&mut session);

    assert_eq!(warning_text(&session), "Clipboard image paste is unavailable in this terminal or desktop session.");
    assert!(take_clipboard_5522_captured().is_empty(), "no 5522 traffic without support");
}

#[test]
fn dispatch_arm_routes_parser_events_to_pending_read() {
    let _guard = Clipboard5522Guard::with_support(true);
    let mut session = app_session_with_input("", 0);
    press_paste_with_failing_os_reader(&mut session);
    let (sender, _receiver) = mpsc::unbounded_channel();

    for packet in [
        "5522;type=read:status=OK",
        &format!("5522;type=read:status=DATA:mime={TEXT_PLAIN_B64};{TEXT_PLAIN_B64}"),
        "5522;type=read:status=DONE",
    ] {
        session.handle_event(CrosstermEvent::Clipboard5522Response(packet.to_string()), &sender, None);
    }

    assert_eq!(warning_text(&session), "No image found in clipboard.");
}

#[test]
fn stray_packets_without_pending_read_are_ignored() {
    let _guard = Clipboard5522Guard::with_support(true);
    let mut session = app_session_with_input("", 0);

    assert!(!feed_packet(&mut session, "5522;type=read:status=OK"));
    assert!(!feed_packet(&mut session, "bogus"));
    assert!(warning_text(&session).is_empty());
    assert!(take_clipboard_5522_captured().is_empty());
}

#[test]
fn unsupported_mime_only_warns_no_image() {
    let _guard = Clipboard5522Guard::with_support(true);
    let mut session = app_session_with_input("", 0);
    press_paste_with_failing_os_reader(&mut session);

    // SVG is text, not a raster image: nothing worth reading.
    feed_list_reply(&mut session, "dGV4dC9wbGFpbiBpbWFnZS9zdmcreG1s");

    assert_eq!(warning_text(&session), "No image found in clipboard.");
    assert_eq!(take_clipboard_5522_captured().len(), 1, "no typed read for SVG-only clipboards");
}

#[test]
fn bmp_only_clipboard_warns_no_image() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as Base64};

    let _guard = Clipboard5522Guard::with_support(true);
    let mut session = app_session_with_input("", 0);
    press_paste_with_failing_os_reader(&mut session);

    // BMP decodes to pixels but no vision API accepts it; the paste contract
    // rejects it instead of attaching a part providers would 400 on.
    let bmp_list = Base64.encode("image/bmp image/tiff");
    feed_list_reply(&mut session, &bmp_list);

    assert_eq!(warning_text(&session), "No image found in clipboard.");
    assert_eq!(take_clipboard_5522_captured().len(), 1, "no typed read for BMP/TIFF-only clipboards");
}
