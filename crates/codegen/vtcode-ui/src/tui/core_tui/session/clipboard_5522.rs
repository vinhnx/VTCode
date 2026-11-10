//! Kitty OSC 5522 clipboard-read fallback for image paste (session side).
//!
//! When a local OS-clipboard image read fails (SSH, containers, headless
//! sessions) and the startup probe reported 5522 support, the image-paste
//! shortcut starts an asynchronous terminal-mediated read instead of warning
//! immediately:
//!
//! 1. send a MIME-list request (permission-free, so no prompt for nothing);
//! 2. on the list reply, read the first `image/*` type when one is present;
//! 3. on the data reply, attach it exactly like a local clipboard image.
//!
//! Replies arrive as `CrosstermEvent::Clipboard5522Response` fork-parser
//! events, one packet per event. [`Session::expire_clipboard_5522_read`] runs
//! on every tick and aborts reads past their deadline (terminal permission
//! prompts need human time, hence generous). Every failure reuses the local
//! image-paste warnings, so users see one consistent clipboard story.
//!
//! Single-flight: starting a read replaces any pending one, and stale or
//! foreign replies are ignored by request-id mismatch.

use std::io::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::STANDARD as Base64};
use vtcode_commons::clipboard_5522::{
    AssembleError, AssembleOutcome, MimeData, ReadAssembler, ReadOptions, clipboard_5522_available,
    generate_request_id, mime_list_request, parse_read_packet, read_request, sanitize_request_id,
};
use vtcode_commons::image::is_supported_image_mime_type;

use super::super::types::{ContentPart, InlineMessageKind, InlineSegment, InlineTextStyle};
use super::Session;
use super::clipboard_image::{ClipboardImageError, image_paste_warning};

/// Human name shown in terminal 5522 permission prompts.
const CLIPBOARD_5522_CLIENT_NAME: &str = "VT Code";

/// Pending-read deadline. Replies land in milliseconds when pre-authorized,
/// but a permission prompt needs human time — expire generously, warn, and
/// let the user retry (authorized on the second attempt).
const CLIPBOARD_5522_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Which request a pending read is awaiting replies for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadPhase {
    /// MIME-list request (`type=read` with a `.` payload).
    MimeList,
    /// Typed image-data request.
    ImageData,
}

#[derive(Debug)]
struct PendingClipboard5522Read {
    phase: ReadPhase,
    assembler: ReadAssembler,
    deadline: Instant,
}

/// 5522 fallback state: a session-stable password plus at most one
/// single-flight pending read.
#[derive(Debug, Default)]
pub(crate) struct Clipboard5522State {
    /// Random per-session password that lets the terminal pre-authorize
    /// future reads (generated lazily on first use so idle sessions pay
    /// nothing and tests stay deterministic until a read starts).
    password: Option<String>,
    pending: Option<PendingClipboard5522Read>,
}

impl Clipboard5522State {
    /// Whether a 5522 read is awaiting replies (a transient tick deadline).
    pub(crate) fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

/// Whether a 5522 read started now could ever complete: replies surface as
/// fork-parser events from the unix TTY reader, so without the vendored fork,
/// a unix TTY, or probe/env support the read would only hang until timeout.
fn clipboard_5522_events_can_arrive() -> bool {
    cfg!(vendored_crossterm) && cfg!(unix) && clipboard_5522_available()
}

fn write_clipboard_5522_query(query: &str) -> bool {
    #[cfg(all(test, vendored_crossterm))]
    if let Some(captured) = clipboard_5522_capture_sink() {
        if let Ok(mut guard) = captured.lock() {
            if let Some(queries) = guard.as_mut() {
                queries.push(query.to_string());
                return true;
            }
        }
    }

    let mut stderr = std::io::stderr();
    stderr.write_all(query.as_bytes()).is_ok() && stderr.flush().is_ok()
}

#[cfg(all(test, vendored_crossterm))]
static CLIPBOARD_5522_CAPTURE: std::sync::OnceLock<std::sync::Mutex<Option<Vec<String>>>> = std::sync::OnceLock::new();

#[cfg(all(test, vendored_crossterm))]
fn clipboard_5522_capture_sink() -> Option<&'static std::sync::Mutex<Option<Vec<String>>>> {
    Some(CLIPBOARD_5522_CAPTURE.get_or_init(|| std::sync::Mutex::new(None)))
}

/// Capture emitted 5522 queries instead of writing them to stderr (tests).
#[cfg(all(test, vendored_crossterm))]
pub(crate) fn set_clipboard_5522_capture(enabled: bool) {
    if let Some(sink) = clipboard_5522_capture_sink() {
        if let Ok(mut guard) = sink.lock() {
            *guard = enabled.then(Vec::new);
        }
    }
}

/// Drain captured 5522 queries (tests).
#[cfg(all(test, vendored_crossterm))]
pub(crate) fn take_clipboard_5522_captured() -> Vec<String> {
    clipboard_5522_capture_sink()
        .and_then(|sink| sink.lock().ok())
        .and_then(|mut guard| guard.as_mut().map(std::mem::take))
        .unwrap_or_default()
}

impl Session {
    /// Start a terminal-mediated clipboard image read (paste fallback).
    ///
    /// Returns whether a read is now pending; `false` keeps the caller's
    /// synchronous warning path. Replaces any pending read (single-flight).
    pub(crate) fn start_clipboard_5522_read(&mut self) -> bool {
        if !clipboard_5522_events_can_arrive() {
            return false;
        }
        let password = self.clipboard_5522.password.get_or_insert_with(generate_request_id).clone();
        // Sanitized once here (identity for generated ids) so the sent query
        // and the assembler's expected id can never disagree.
        let id = sanitize_request_id(&generate_request_id());
        let query = mime_list_request(&ReadOptions {
            id: &id,
            password: Some(&password),
            name: Some(CLIPBOARD_5522_CLIENT_NAME),
            primary: false,
        });
        if !write_clipboard_5522_query(&query) {
            return false;
        }
        self.clipboard_5522.pending = Some(PendingClipboard5522Read {
            phase: ReadPhase::MimeList,
            assembler: ReadAssembler::new(&id),
            deadline: Instant::now() + CLIPBOARD_5522_READ_TIMEOUT,
        });
        true
    }

    /// Drive a pending read with one fork-parser reply packet.
    ///
    /// Packets without a pending read, or with a foreign request id, are
    /// ignored. Terminal errors, malformed packets, and oversized data abort
    /// the read with the local image-paste warning. Returns whether an image
    /// was attached (input text changed), so app-level input triggers can
    /// refresh.
    #[cfg_attr(not(vendored_crossterm), allow(dead_code))]
    pub(crate) fn handle_clipboard_5522_packet(&mut self, packet: &str) -> bool {
        if self.clipboard_5522.pending.is_none() {
            return false;
        }
        let parsed = match parse_read_packet(packet) {
            Ok(parsed) => parsed,
            Err(_) => {
                self.abort_clipboard_5522_read(ClipboardImageError::ClipboardUnavailable);
                return false;
            }
        };
        let Some(pending) = self.clipboard_5522.pending.as_mut() else {
            return false;
        };
        match pending.assembler.push(&parsed) {
            Ok(AssembleOutcome::NeedMore) => false,
            Ok(AssembleOutcome::Complete(mime_data)) => {
                let phase = pending.phase;
                self.finish_clipboard_5522_phase(phase, mime_data)
            }
            // Another client's reply on a shared multiplexer line; keep
            // waiting for ours instead of failing the read.
            Err(AssembleError::IdMismatch) => false,
            Err(_) => {
                self.abort_clipboard_5522_read(ClipboardImageError::ClipboardUnavailable);
                false
            }
        }
    }

    /// Abort a 5522 read past its deadline; runs on every tick.
    pub(crate) fn expire_clipboard_5522_read(&mut self) {
        let expired = self
            .clipboard_5522
            .pending
            .as_ref()
            .is_some_and(|pending| clipboard_5522_read_expired(pending.deadline, Instant::now()));
        if expired {
            self.abort_clipboard_5522_read(ClipboardImageError::ClipboardUnavailable);
        }
    }

    #[cfg_attr(not(vendored_crossterm), allow(dead_code))]
    fn finish_clipboard_5522_phase(&mut self, phase: ReadPhase, mime_data: Vec<MimeData>) -> bool {
        match phase {
            ReadPhase::MimeList => {
                let list = mime_data
                    .first()
                    .map(|data| String::from_utf8_lossy(&data.decoded_bytes).into_owned())
                    .unwrap_or_default();
                match select_clipboard_image_mime(&list) {
                    Some(mime) => {
                        self.request_clipboard_5522_image(&mime);
                        false
                    }
                    None => {
                        self.abort_clipboard_5522_read(ClipboardImageError::NoImage);
                        false
                    }
                }
            }
            ReadPhase::ImageData => {
                let normalized = mime_data
                    .first()
                    .filter(|data| !data.decoded_bytes.is_empty())
                    .and_then(|data| normalize_clipboard_image(data.mime.as_str(), &data.decoded_bytes));
                let Some((mime, bytes)) = normalized else {
                    self.abort_clipboard_5522_read(ClipboardImageError::NoImage);
                    return false;
                };
                self.attach_clipboard_5522_image(&mime, &bytes);
                true
            }
        }
    }

    #[cfg_attr(not(vendored_crossterm), allow(dead_code))]
    fn request_clipboard_5522_image(&mut self, mime: &str) {
        // Fresh id per request so list and image replies never cross; the
        // password stays session-stable so terminal allowlists hold.
        // Sanitized once (identity for generated ids) to match the assembler.
        let id = sanitize_request_id(&generate_request_id());
        let password = self.clipboard_5522.password.get_or_insert_with(generate_request_id).clone();
        let query = read_request(
            &[mime],
            &ReadOptions {
                id: &id,
                password: Some(&password),
                name: Some(CLIPBOARD_5522_CLIENT_NAME),
                primary: false,
            },
        );
        if !write_clipboard_5522_query(&query) {
            self.abort_clipboard_5522_read(ClipboardImageError::ClipboardUnavailable);
            return;
        }
        self.clipboard_5522.pending = Some(PendingClipboard5522Read {
            phase: ReadPhase::ImageData,
            assembler: ReadAssembler::new(&id),
            deadline: Instant::now() + CLIPBOARD_5522_READ_TIMEOUT,
        });
    }

    #[cfg_attr(not(vendored_crossterm), allow(dead_code))]
    fn attach_clipboard_5522_image(&mut self, mime: &str, bytes: &[u8]) {
        self.clipboard_5522.pending = None;
        let image = ContentPart::image(Base64.encode(bytes), mime);
        if let Some(attachment_number) = self.input_manager.push_attachment(image) {
            self.input_manager.insert_text(&format!("[Image #{attachment_number}]"));
        }
        // Mirror the synchronous image-paste path: collapse state follows
        // attachment changes, and app input triggers refresh via the
        // packet handler's return value.
        self.set_input_compact_mode(self.input_compact_placeholder().is_some());
        self.mark_dirty();
    }

    fn abort_clipboard_5522_read(&mut self, error: ClipboardImageError) {
        self.clipboard_5522.pending = None;
        self.push_clipboard_5522_warning(error);
    }

    fn push_clipboard_5522_warning(&mut self, error: ClipboardImageError) {
        self.push_line(
            InlineMessageKind::Warning,
            vec![InlineSegment {
                text: image_paste_warning(error).to_string(),
                style: Arc::new(InlineTextStyle::default()),
            }],
        );
        self.request_transcript_clear();
        self.mark_dirty();
    }

    /// Force a pending read's deadline (vendored-only fallback tests).
    #[cfg(all(test, vendored_crossterm))]
    pub(crate) fn set_clipboard_5522_deadline_for_tests(&mut self, deadline: Instant) {
        if let Some(pending) = self.clipboard_5522.pending.as_mut() {
            pending.deadline = deadline;
        }
    }
}

/// Whether `now` has reached a pending read's `deadline` (inclusive).
fn clipboard_5522_read_expired(deadline: Instant, now: Instant) -> bool {
    now >= deadline
}

/// Pick a clipboard MIME type worth reading: the first vision-API format.
///
/// Anything outside [`is_supported_image_mime_type`] (BMP, TIFF, SVG, ...)
/// is rejected, matching the documented paste contract ("attached images
/// must be JPEG, PNG, GIF, or WebP"). Received spelling is echoed back so
/// the terminal matches its own advertisement.
fn select_clipboard_image_mime(list: &str) -> Option<String> {
    list.split_whitespace()
        .find(|mime| is_supported_image_mime_type(&mime.to_ascii_lowercase()))
        .map(str::to_string)
}

/// Normalize clipboard image bytes to a vision-API MIME type.
///
/// Supported formats pass through lowercased; anything else (BMP, TIFF, SVG)
/// yields `None` so the caller warns instead of attaching a part providers
/// would reject with `400 invalid_value`.
fn normalize_clipboard_image(mime: &str, bytes: &[u8]) -> Option<(String, Vec<u8>)> {
    let normalized = mime.to_ascii_lowercase();
    if !is_supported_image_mime_type(&normalized) {
        return None;
    }
    Some((normalized, bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_expiry_boundary_is_inclusive() {
        let deadline = Instant::now();
        let just_before = deadline
            .checked_sub(Duration::from_nanos(1))
            .expect("deadline should predate now");
        assert!(!clipboard_5522_read_expired(deadline, just_before));
        assert!(clipboard_5522_read_expired(deadline, deadline));
        assert!(clipboard_5522_read_expired(deadline, deadline + Duration::from_secs(30)));
    }

    #[test]
    fn mime_selection_picks_first_supported_format() {
        assert_eq!(select_clipboard_image_mime("text/plain image/png"), Some("image/png".to_string()));
        assert_eq!(select_clipboard_image_mime("image/gif text/plain"), Some("image/gif".to_string()));
        // Case-insensitive match, received spelling echoed back.
        assert_eq!(select_clipboard_image_mime("IMAGE/PNG"), Some("IMAGE/PNG".to_string()));
    }

    #[test]
    fn mime_selection_rejects_unsupported_raster_and_text() {
        assert_eq!(select_clipboard_image_mime(""), None);
        assert_eq!(select_clipboard_image_mime("text/plain"), None);
        assert_eq!(select_clipboard_image_mime("text/plain image/svg+xml"), None);
        assert_eq!(select_clipboard_image_mime("image/bmp image/tiff"), None);
        assert_eq!(select_clipboard_image_mime("application/octet-stream"), None);
    }

    #[test]
    fn normalize_passes_supported_bytes_through_lowercased() {
        let bytes = vec![1, 2, 3, 4];
        assert_eq!(normalize_clipboard_image("image/png", &bytes), Some(("image/png".to_string(), bytes.clone())));
        assert_eq!(normalize_clipboard_image("IMAGE/JPEG", &bytes), Some(("image/jpeg".to_string(), bytes)));
    }

    #[test]
    fn normalize_rejects_unsupported_mimes() {
        assert_eq!(normalize_clipboard_image("image/svg+xml", b"<svg/>"), None);
        assert_eq!(normalize_clipboard_image("image/bmp", &[1, 2, 3]), None);
        assert_eq!(normalize_clipboard_image("image/tiff", &[1, 2, 3]), None);
        assert_eq!(normalize_clipboard_image("text/plain", b"hi"), None);
    }
}
