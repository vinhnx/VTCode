//! Kitty OSC 5522 rich-clipboard protocol: request builders, reply parsing, capability hint.
//!
//! Spec: <https://sw.kovidgoyal.net/kitty/clipboard/>
//!
//! VT Code implements the **read path only**: the TUI falls back to it when a
//! local OS-clipboard image read fails (SSH, containers, headless sessions).
//! The write path (`type=write`/`wdata`/`walias`) and paste-event mode
//! (`CSI ? 5522 h`) are deliberately out of scope — terminal copy stays on
//! plain OSC 52 and paste stays on bracketed paste plus this read fallback.
//!
//! Two consumers share this module:
//!
//! - the startup color probe ([`crate::utils`] callers in `vtcode-core`)
//!   folds a DECRQM 5522 query into its existing OSC batch and caches the
//!   result with [`set_clipboard_5522_hint`];
//! - the TUI session (Phase 1) sends [`read_request`] on image-paste
//!   fallback, receives one reply packet per fork-parser event, and feeds
//!   them to [`ReadAssembler`].
//!
//! All parsing is fail-closed: malformed packets, id mismatches, oversized
//! payloads, and terminal error statuses abort the read instead of yielding
//! partial data.

use std::sync::atomic::{AtomicU8, Ordering};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_ENGINE};
use compact_str::CompactString;
use thiserror::Error;

use crate::ansi_codes::{OSC, ST};
use crate::utils::parse_bool_env_value;

/// Query for 5522 support: `CSI ? 5522 $ p` (DECRQM).
pub const DECRQM_5522_QUERY: &str = "\x1b[?5522$p";

/// Prefix of a DECRPM reply to [`DECRQM_5522_QUERY`]: `CSI ? 5522 ; Ps $ y`.
pub const DECRPM_5522_PREFIX: &str = "\x1b[?5522;";

/// Upper bound for one 5522 reply packet (base64 payload plus metadata).
///
/// The spec chunks DATA payloads at 4096 decoded bytes (~5464 base64 chars),
/// so 16 KiB accepts any conforming packet while bounding a malicious or
/// buggy terminal's per-packet memory. Packets beyond this are rejected.
pub const MAX_PACKET_BYTES: usize = 16 * 1024;

/// Upper bound for the total decoded bytes of one clipboard read.
///
/// Clipboard images are small; anything beyond 16 MiB aborts the read
/// ([`AssembleError::TooLarge`]) instead of buffering unbounded input.
pub const MAX_READ_BYTES: usize = 16 * 1024 * 1024;

/// MIME-list probe payload: a single period, base64-encoded (`Lg==`).
const MIME_LIST_PROBE_B64: &str = "Lg==";

/// Request id charset from the spec: `[a-zA-Z0-9-_+.]`.
fn is_request_id_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'+' | b'.')
}

/// Shared options for 5522 read requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOptions<'a> {
    /// Caller-chosen correlation id; sanitized by [`sanitize_request_id`].
    pub id: &'a str,
    /// Session password that lets the terminal pre-authorize future reads.
    /// Only sent together with [`ReadOptions::name`].
    pub password: Option<&'a str>,
    /// Human-friendly client name shown in terminal permission prompts.
    pub name: Option<&'a str>,
    /// Read from the primary selection instead of the clipboard.
    pub primary: bool,
}

/// Strip characters outside the spec's request-id charset.
///
/// Terminals strip invalid characters before echoing the id back, so
/// sanitizing client-side keeps our correlation comparison exact.
#[must_use]
pub fn sanitize_request_id(id: &str) -> String {
    id.bytes().filter(|byte| is_request_id_byte(*byte)).map(char::from).collect()
}

/// Generate a random correlation id inside the spec's charset.
#[must_use]
pub fn generate_request_id() -> String {
    use rand::{RngExt, distr::Alphanumeric};

    rand::rng().sample_iter(&Alphanumeric).take(16).map(char::from).collect()
}

fn encode_options(options: &ReadOptions, metadata: &mut String) {
    let id = sanitize_request_id(options.id);
    if !id.is_empty() {
        metadata.push_str(":id=");
        metadata.push_str(&id);
    }
    // A password without a human name is equivalent to no password; the
    // terminal must treat the request as though it had none. Enforce that
    // here so callers cannot accidentally weaken the request.
    if let (Some(password), Some(name)) = (options.password, options.name) {
        metadata.push_str(":pw=");
        metadata.push_str(&BASE64_ENGINE.encode(password.as_bytes()));
        metadata.push_str(":name=");
        metadata.push_str(&BASE64_ENGINE.encode(name.as_bytes()));
    }
    if options.primary {
        metadata.push_str(":loc=primary");
    }
}

/// Build a MIME-list request: payload is base64(`.`).
///
/// Listing is permission-free, so the session lists first and only reads
/// when an image MIME type is actually present — no prompt for nothing.
#[must_use]
pub fn mime_list_request(options: &ReadOptions) -> String {
    let mut metadata = String::from("type=read");
    encode_options(options, &mut metadata);
    format!("{OSC}5522;{metadata};{MIME_LIST_PROBE_B64}{ST}")
}

/// Build a typed read request for space-separated `mimes`.
#[must_use]
pub fn read_request(mimes: &[&str], options: &ReadOptions) -> String {
    let mut metadata = String::from("type=read");
    encode_options(options, &mut metadata);
    let payload = BASE64_ENGINE.encode(mimes.join(" ").as_bytes());
    format!("{OSC}5522;{metadata};{payload}{ST}")
}

/// Parse a DECRPM 5522 reply out of a mixed probe buffer.
///
/// `Ps` 0 or 4 means unsupported, 1/2/3 means supported; anything else is
/// ignored and the last valid report wins. Returns `None` when the terminal
/// sent no 5522 report (treat as unsupported — fail closed).
#[must_use]
pub fn parse_decrpm_5522(response: &[u8]) -> Option<bool> {
    let decoded = String::from_utf8_lossy(response);
    let mut supported = None;
    for (pos, _) in decoded.match_indices(DECRPM_5522_PREFIX) {
        let Some(after) = decoded.get(pos + DECRPM_5522_PREFIX.len()..) else {
            continue;
        };
        // DECRPM ends with `$ y`; `split_once` fails when the reply is
        // truncated, which we treat as no report.
        let Some((payload, _)) = after.split_once("$y") else {
            continue;
        };
        match payload {
            "1" | "2" | "3" => supported = Some(true),
            "0" | "4" => supported = Some(false),
            _ => {}
        }
    }
    supported
}

/// Terminal error status for a 5522 read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadErrorCode {
    /// Requested clipboard location is unavailable (`loc=primary` on a
    /// system without a primary selection).
    NoSys,
    /// Permission denied by the system or the user.
    Perm,
    /// Temporary failure (another multiplexer client holds the clipboard).
    Busy,
    /// Forward-compatibility: a status value this client does not know.
    /// Treated as a terminal failure, never ignored.
    Unknown(CompactString),
}

impl ReadErrorCode {
    fn from_status(status: &str) -> Self {
        match status {
            "ENOSYS" => Self::NoSys,
            "EPERM" => Self::Perm,
            "EBUSY" => Self::Busy,
            other => Self::Unknown(CompactString::new(other)),
        }
    }

    /// Short lowercase name for terminal-error reports.
    fn name(&self) -> CompactString {
        match self {
            Self::NoSys => CompactString::const_new("unavailable"),
            Self::Perm => CompactString::const_new("denied"),
            Self::Busy => CompactString::const_new("busy"),
            Self::Unknown(code) => code.clone(),
        }
    }
}

/// Status of one parsed 5522 read-reply packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadStatus {
    /// `status=OK`: terminal accepted the read; DATA packets follow.
    Ok,
    /// `status=DATA`: one chunk of one MIME type's base64 payload.
    Data,
    /// `status=DONE`: end of transmission.
    Done,
    /// `status=<ERRORCODE>`: terminal aborted the read.
    Error(ReadErrorCode),
}

/// One parsed 5522 read-reply packet (OSC framing already stripped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadPacket {
    /// Packet status.
    pub status: ReadStatus,
    /// Decoded MIME type (`DATA` packets only).
    pub mime: Option<CompactString>,
    /// Raw base64 chunk (`DATA` packets only); decoded at assembly time
    /// because chunks concatenate before decoding.
    pub data_b64: String,
    /// Echoed request id, when the terminal sent one back.
    pub id: Option<CompactString>,
}

/// A single packet failed to parse.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PacketError {
    /// Packet does not start with `5522;`.
    #[error("not a 5522 packet")]
    Not5522,
    /// Metadata has no `type=` pair.
    #[error("5522 packet without type")]
    MissingType,
    /// `type=` is not `read`.
    #[error("unexpected 5522 type: {0}")]
    UnexpectedType(CompactString),
    /// Metadata has no `status=` pair.
    #[error("5522 packet without status")]
    MissingStatus,
    /// `DATA` packet without a `mime=` pair.
    #[error("5522 data packet without mime")]
    MissingMime,
    /// `mime=` value is not valid base64 or not UTF-8.
    #[error("5522 data packet with undecodable mime")]
    InvalidMime,
    /// Packet exceeds [`MAX_PACKET_BYTES`].
    #[error("5522 packet exceeds size limit")]
    TooLarge,
}

/// Strict base64 predicate shared by parser and tests: standard alphabet,
/// correct padding shape, nothing silently discarded (spec section on
/// payload encoding forbids dropping whitespace or bad characters).
fn is_strict_base64(payload: &str) -> bool {
    !payload.is_empty()
        && payload.len().is_multiple_of(4)
        && payload
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

/// Parse one 5522 reply packet with OSC framing already stripped.
///
/// `packet` is the inner `5522;metadata;payload` content: the fork parser
/// owns framing (terminator scanning, the 16 KiB packet cap) and hands the
/// session exactly one complete packet per event. Unknown metadata keys are
/// ignored for forward compatibility; unknown `status=` values become
/// [`ReadErrorCode::Unknown`] so the read fails instead of hanging.
///
/// # Errors
///
/// Returns [`PacketError`] when the packet is malformed (see variants).
pub fn parse_read_packet(packet: &str) -> Result<ReadPacket, PacketError> {
    // Defense in depth with the fork's framing cap: never parse an
    // over-sized packet even if a future event source skips the check.
    if packet.len() > MAX_PACKET_BYTES {
        return Err(PacketError::TooLarge);
    }
    let Some(inner) = packet.strip_prefix("5522;") else {
        return Err(PacketError::Not5522);
    };
    // Metadata and payload split on the FIRST `;`: base64 never contains it.
    let (metadata, payload) = inner.split_once(';').unwrap_or((inner, ""));

    let mut type_seen = false;
    let mut status_raw: Option<&str> = None;
    let mut mime_raw: Option<&str> = None;
    let mut id_raw: Option<&str> = None;
    for pair in metadata.split(':') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "type" => {
                if value != "read" {
                    return Err(PacketError::UnexpectedType(CompactString::new(value)));
                }
                type_seen = true;
            }
            "status" => status_raw = Some(value),
            "mime" => mime_raw = Some(value),
            "id" => id_raw = Some(value),
            _ => {}
        }
    }
    if !type_seen {
        return Err(PacketError::MissingType);
    }
    let Some(status_raw) = status_raw else {
        return Err(PacketError::MissingStatus);
    };

    let status = match status_raw {
        "OK" => ReadStatus::Ok,
        "DATA" => ReadStatus::Data,
        "DONE" => ReadStatus::Done,
        other => ReadStatus::Error(ReadErrorCode::from_status(other)),
    };

    let mime = if matches!(status, ReadStatus::Data) {
        let Some(mime_raw) = mime_raw else {
            return Err(PacketError::MissingMime);
        };
        let mime = BASE64_ENGINE
            .decode(mime_raw)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(CompactString::new)
            .ok_or(PacketError::InvalidMime)?;
        Some(mime)
    } else {
        None
    };

    let is_data = matches!(status, ReadStatus::Data);
    Ok(ReadPacket {
        status,
        mime,
        data_b64: if is_data { payload.to_string() } else { String::new() },
        id: id_raw.map(CompactString::new),
    })
}

/// One fully assembled MIME payload from a 5522 read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeData {
    /// Decoded MIME type, e.g. `image/png`.
    pub mime: CompactString,
    /// Concatenated decoded bytes of all DATA chunks for this type.
    pub decoded_bytes: Vec<u8>,
}

/// Outcome of feeding one packet to [`ReadAssembler::push`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssembleOutcome {
    /// More packets needed.
    NeedMore,
    /// `DONE` seen; all MIME payloads in transmission order.
    Complete(Vec<MimeData>),
}

/// A 5522 read failed while assembling packets.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AssembleError {
    /// Terminal aborted the read with an error status.
    #[error("clipboard read failed: {0}")]
    Terminal(CompactString),
    /// Reply id differs from the request id.
    #[error("clipboard reply id mismatch")]
    IdMismatch,
    /// Packet arrived out of protocol order (DATA before OK, second OK,
    /// packet after DONE).
    #[error("clipboard reply out of order")]
    OutOfOrder,
    /// Total decoded bytes exceeded the assembler cap.
    #[error("clipboard data exceeds size limit")]
    TooLarge,
    /// A DATA chunk is not strict base64 (standard alphabet, padded shape,
    /// nothing silently discarded).
    #[error("clipboard chunk is not valid base64")]
    InvalidBase64,
}

impl AssembleError {
    fn terminal(code: &ReadErrorCode) -> Self {
        Self::Terminal(code.name())
    }
}

/// Incremental assembler for one 5522 read: `OK (DATA*) DONE`.
///
/// DATA chunks for one MIME type arrive sequentially, then the next type's
/// chunks; each chunk is individually padded base64 of at most 4096 decoded
/// bytes, so chunks decode independently and decoded bytes concatenate.
/// A single assembler handles exactly one read and is discarded after
/// [`AssembleOutcome::Complete`] or any error.
#[derive(Debug)]
pub struct ReadAssembler {
    expected_id: String,
    max_bytes: usize,
    started: bool,
    finished: bool,
    current_mime: Option<CompactString>,
    current_bytes: Vec<u8>,
    completed: Vec<MimeData>,
    total_bytes: usize,
}

impl ReadAssembler {
    /// New assembler capped at [`MAX_READ_BYTES`] total decoded bytes.
    #[must_use]
    pub fn new(expected_id: &str) -> Self {
        Self::with_max_bytes(expected_id, MAX_READ_BYTES)
    }

    /// New assembler with an explicit decoded-bytes cap (tests use a small cap
    /// instead of allocating megabytes).
    #[must_use]
    pub fn with_max_bytes(expected_id: &str, max_bytes: usize) -> Self {
        Self {
            expected_id: expected_id.to_string(),
            max_bytes,
            started: false,
            finished: false,
            current_mime: None,
            current_bytes: Vec::new(),
            completed: Vec::new(),
            total_bytes: 0,
        }
    }

    fn check_id(&self, packet: &ReadPacket) -> Result<(), AssembleError> {
        // Terminals must echo the id, but an absent id is tolerated for
        // interop (single-flight reads make cross-talk unlikely). A
        // PRESENT but different id is a hard failure.
        if let Some(id) = packet.id.as_deref()
            && id != self.expected_id
        {
            return Err(AssembleError::IdMismatch);
        }
        Ok(())
    }

    fn flush_current(&mut self) {
        if let Some(mime) = self.current_mime.take() {
            let decoded_bytes = std::mem::take(&mut self.current_bytes);
            self.completed.push(MimeData { mime, decoded_bytes });
        }
    }

    /// Feed one parsed packet; returns the read outcome.
    ///
    /// # Errors
    ///
    /// Returns [`AssembleError`] on id mismatch, out-of-order packets,
    /// terminal error statuses, oversized data, or invalid base64.
    pub fn push(&mut self, packet: &ReadPacket) -> Result<AssembleOutcome, AssembleError> {
        self.check_id(packet)?;
        if self.finished {
            return Err(AssembleError::OutOfOrder);
        }
        match &packet.status {
            ReadStatus::Ok => {
                if self.started {
                    return Err(AssembleError::OutOfOrder);
                }
                self.started = true;
                Ok(AssembleOutcome::NeedMore)
            }
            ReadStatus::Data => {
                if !self.started {
                    return Err(AssembleError::OutOfOrder);
                }
                let Some(mime) = packet.mime.as_ref() else {
                    return Err(AssembleError::OutOfOrder);
                };
                if !packet.data_b64.is_empty() && !is_strict_base64(&packet.data_b64) {
                    return Err(AssembleError::InvalidBase64);
                }
                let decoded = BASE64_ENGINE
                    .decode(packet.data_b64.as_bytes())
                    .ok()
                    .ok_or(AssembleError::InvalidBase64)?;
                self.total_bytes = self.total_bytes.saturating_add(decoded.len());
                if self.total_bytes > self.max_bytes {
                    return Err(AssembleError::TooLarge);
                }
                if self.current_mime.as_ref() != Some(mime) {
                    self.flush_current();
                    self.current_mime = Some(mime.clone());
                }
                self.current_bytes.extend_from_slice(&decoded);
                Ok(AssembleOutcome::NeedMore)
            }
            ReadStatus::Done => {
                if !self.started {
                    return Err(AssembleError::OutOfOrder);
                }
                self.flush_current();
                self.finished = true;
                Ok(AssembleOutcome::Complete(std::mem::take(&mut self.completed)))
            }
            ReadStatus::Error(code) => Err(AssembleError::terminal(code)),
        }
    }
}

const HINT_FALSE: u8 = 0;
const HINT_TRUE: u8 = 1;
const HINT_UNSET: u8 = 2;

static CLIPBOARD_5522_HINT: AtomicU8 = AtomicU8::new(HINT_UNSET);

/// Cache the startup probe's 5522 verdict (`None` clears the hint).
///
/// Populated once by the terminal probe in `vtcode-core`, which folds a
/// DECRQM 5522 query into its existing OSC batch; `None` (no report) leaves
/// the hint unset and reads as unsupported.
pub fn set_clipboard_5522_hint(value: Option<bool>) {
    let encoded = match value {
        Some(true) => HINT_TRUE,
        Some(false) => HINT_FALSE,
        None => HINT_UNSET,
    };
    CLIPBOARD_5522_HINT.store(encoded, Ordering::Relaxed);
}

/// Cached startup-probe verdict, if the probe produced one.
#[must_use]
pub fn clipboard_5522_hint() -> Option<bool> {
    match CLIPBOARD_5522_HINT.load(Ordering::Relaxed) {
        HINT_TRUE => Some(true),
        HINT_FALSE => Some(false),
        _ => None,
    }
}

/// Whether the session may attempt 5522 clipboard reads.
///
/// Resolution order: `VTCODE_CLIPBOARD_5522` env override (canonical
/// tri-state values), then the startup-probe hint, defaulting to `false`.
/// Unknown terminals fail closed — no 5522 traffic without evidence.
#[must_use]
pub fn clipboard_5522_available() -> bool {
    if let Some(override_value) = std::env::var("VTCODE_CLIPBOARD_5522")
        .ok()
        .and_then(|value| parse_bool_env_value(&value))
    {
        return override_value;
    }
    clipboard_5522_hint().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(id: &str) -> ReadOptions<'_> {
        ReadOptions { id, password: None, name: None, primary: false }
    }

    #[test]
    fn sanitize_request_id_keeps_spec_charset_only() {
        assert_eq!(sanitize_request_id("abcXYZ019-_+."), "abcXYZ019-_+.");
        assert_eq!(sanitize_request_id("a b;c=d:e/f\\g"), "abcdefg");
        assert_eq!(sanitize_request_id("héllo→"), "hllo");
        assert_eq!(sanitize_request_id(""), "");
    }

    #[test]
    fn generated_request_id_is_charset_clean() {
        for _ in 0..32 {
            let id = generate_request_id();
            assert_eq!(id.len(), 16);
            assert_eq!(sanitize_request_id(&id), id);
        }
    }

    #[test]
    fn mime_list_request_is_byte_exact() {
        let request = mime_list_request(&options("test-id_1.2+x"));
        assert_eq!(request, "\x1b]5522;type=read:id=test-id_1.2+x;Lg==\x1b\\");
    }

    #[test]
    fn read_request_is_byte_exact_with_all_options() {
        let request = read_request(
            &["text/plain", "image/png"],
            &ReadOptions {
                id: "a b",
                password: Some("p"),
                name: Some("n"),
                primary: true,
            },
        );
        assert_eq!(
            request,
            "\x1b]5522;type=read:id=ab:pw=cA==:name=bg==:loc=primary;dGV4dC9wbGFpbiBpbWFnZS9wbmc=\x1b\\"
        );
    }

    #[test]
    fn password_without_name_is_dropped() {
        let with_name = read_request(
            &["a"],
            &ReadOptions {
                id: "i",
                password: Some("p"),
                name: Some("n"),
                primary: false,
            },
        );
        let without_name = read_request(
            &["a"],
            &ReadOptions {
                id: "i",
                password: Some("p"),
                name: None,
                primary: false,
            },
        );
        assert!(with_name.contains(":pw=cA==:name=bg==;"), "{with_name:?}");
        assert_eq!(without_name, "\x1b]5522;type=read:id=i;YQ==\x1b\\");
    }

    #[test]
    fn primary_flag_toggles_loc_pair() {
        let primary = read_request(&["a"], &ReadOptions { id: "i", password: None, name: None, primary: true });
        let clipboard = read_request(&["a"], &options("i"));
        assert!(primary.contains(":loc=primary;"), "{primary:?}");
        assert!(!clipboard.contains("loc="), "{clipboard:?}");
    }

    #[test]
    fn decrpm_reports_supported_for_set_modes() {
        for payload in ["\x1b[?5522;1$y", "\x1b[?5522;2$y", "\x1b[?5522;3$y"] {
            assert_eq!(parse_decrpm_5522(payload.as_bytes()), Some(true), "{payload:?}");
        }
    }

    #[test]
    fn decrpm_reports_unsupported_for_unset_modes() {
        for payload in ["\x1b[?5522;0$y", "\x1b[?5522;4$y"] {
            assert_eq!(parse_decrpm_5522(payload.as_bytes()), Some(false), "{payload:?}");
        }
    }

    #[test]
    fn decrpm_ignores_absent_and_malformed_reports() {
        assert_eq!(parse_decrpm_5522(b""), None);
        assert_eq!(parse_decrpm_5522(b"\x1b[?62;22;52c"), None);
        assert_eq!(parse_decrpm_5522(b"\x1b[?5522;1$n"), None);
        assert_eq!(parse_decrpm_5522(b"\x1b[?5522;12$y"), None);
        assert_eq!(parse_decrpm_5522(b"\x1b[?5522;1$z"), None);
        assert_eq!(parse_decrpm_5522(b"\x1b[?5522;1"), None);
    }

    #[test]
    fn decrpm_parses_inside_mixed_probe_buffer_and_last_wins() {
        let buffer = "\x1b]10;rgb:dddd/dddd/dddd\x1b\\\x1b[?997;1n\x1b[?5522;0$y\x1b[?62;4c";
        assert_eq!(parse_decrpm_5522(buffer.as_bytes()), Some(false));
        let flipped = "\x1b[?5522;0$y\x1b[?62;4c\x1b[?5522;1$y";
        assert_eq!(parse_decrpm_5522(flipped.as_bytes()), Some(true));
    }

    #[test]
    fn packet_parses_ok_done_and_error_shapes() {
        let ok = parse_read_packet("5522;type=read:status=OK:id=abc").expect("ok packet");
        assert_eq!(ok.status, ReadStatus::Ok);
        assert_eq!(ok.id.as_deref(), Some("abc"));
        assert!(ok.mime.is_none());

        let done = parse_read_packet("5522;type=read:status=DONE").expect("done packet");
        assert_eq!(done.status, ReadStatus::Done);

        let error = parse_read_packet("5522;type=read:status=EPERM:id=abc").expect("error packet");
        assert_eq!(error.status, ReadStatus::Error(ReadErrorCode::Perm));

        let unknown = parse_read_packet("5522;type=read:status=EWEIRD").expect("unknown status");
        assert!(matches!(unknown.status, ReadStatus::Error(ReadErrorCode::Unknown(_))));
    }

    #[test]
    fn packet_parses_data_with_decoded_mime() {
        let packet = parse_read_packet("5522;type=read:status=DATA:mime=aW1hZ2UvcG5n;YWI=").expect("data packet");
        assert_eq!(packet.status, ReadStatus::Data);
        assert_eq!(packet.mime.as_deref(), Some("image/png"));
        assert_eq!(packet.data_b64, "YWI=");
    }

    #[test]
    fn packet_rejects_malformed_shapes() {
        assert_eq!(parse_read_packet("52;c;Zm9v"), Err(PacketError::Not5522));
        assert_eq!(parse_read_packet("5522;status=OK"), Err(PacketError::MissingType));
        assert!(matches!(parse_read_packet("5522;type=write:status=DONE"), Err(PacketError::UnexpectedType(_))));
        assert_eq!(parse_read_packet("5522;type=read"), Err(PacketError::MissingStatus));
        assert_eq!(parse_read_packet("5522;type=read:status=DATA;YWI="), Err(PacketError::MissingMime));
        assert_eq!(parse_read_packet("5522;type=read:status=DATA:mime=!!!;YWI="), Err(PacketError::InvalidMime));
    }

    #[test]
    fn packet_rejects_oversized_payload_at_cap_boundary() {
        let oversized = format!("5522;type=read:status=DATA:mime=YQ==;{}", "A".repeat(MAX_PACKET_BYTES));
        assert_eq!(parse_read_packet(&oversized), Err(PacketError::TooLarge));

        // Exactly MAX_PACKET_BYTES still parses (unknown keys are ignored).
        let head = "5522;type=read:status=OK:pad=";
        let exact = format!("{head}{}", "A".repeat(MAX_PACKET_BYTES - head.len()));
        assert_eq!(exact.len(), MAX_PACKET_BYTES);
        assert!(matches!(parse_read_packet(&exact), Ok(ReadPacket { status: ReadStatus::Ok, .. })));
    }

    #[test]
    fn packet_ignores_unknown_metadata_keys() {
        let packet = parse_read_packet("5522;type=read:status=OK:future=1:id=z").expect("forward compat");
        assert_eq!(packet.status, ReadStatus::Ok);
        assert_eq!(packet.id.as_deref(), Some("z"));
    }

    #[test]
    fn strict_base64_predicate_matches_spec() {
        assert!(is_strict_base64("YWI="));
        assert!(is_strict_base64("aW1hZ2UvcG5n"));
        assert!(!is_strict_base64(""));
        assert!(!is_strict_base64("YWI"));
        assert!(!is_strict_base64("YWI=="));
        assert!(!is_strict_base64("YW I="));
        assert!(!is_strict_base64("YWI=\n"));
        assert!(!is_strict_base64("YW-I="));
    }

    fn data_packet(mime_b64: &str, chunk_b64: &str) -> ReadPacket {
        parse_read_packet(&format!("5522;type=read:status=DATA:mime={mime_b64};{chunk_b64}")).expect("data packet")
    }

    #[test]
    fn assembler_concatenates_chunks_per_mime_in_order() {
        let mut assembler = ReadAssembler::new("id-1");
        let ok = parse_read_packet("5522;type=read:status=OK:id=id-1").expect("ok");
        assert_eq!(assembler.push(&ok), Ok(AssembleOutcome::NeedMore));

        let png = "aW1hZ2UvcG5n";
        let text = "dGV4dC9wbGFpbg==";
        assert_eq!(assembler.push(&data_packet(png, "YWI=")), Ok(AssembleOutcome::NeedMore));
        assert_eq!(assembler.push(&data_packet(png, "Y2Q=")), Ok(AssembleOutcome::NeedMore));
        assert_eq!(assembler.push(&data_packet(text, "eA==")), Ok(AssembleOutcome::NeedMore));

        let done = parse_read_packet("5522;type=read:status=DONE:id=id-1").expect("done");
        let outcome = assembler.push(&done).expect("complete");
        assert_eq!(
            outcome,
            AssembleOutcome::Complete(vec![
                MimeData {
                    mime: CompactString::new("image/png"),
                    decoded_bytes: b"abcd".to_vec()
                },
                MimeData {
                    mime: CompactString::new("text/plain"),
                    decoded_bytes: b"x".to_vec()
                },
            ])
        );
    }

    #[test]
    fn assembler_rejects_out_of_order_packets() {
        let mut assembler = ReadAssembler::new("id-1");
        // DATA before OK.
        assert_eq!(assembler.push(&data_packet("YQ==", "YWI=")), Err(AssembleError::OutOfOrder));

        let ok = parse_read_packet("5522;type=read:status=OK").expect("ok");
        assert_eq!(assembler.push(&ok), Ok(AssembleOutcome::NeedMore));
        // Second OK.
        assert_eq!(assembler.push(&ok), Err(AssembleError::OutOfOrder));

        let done = parse_read_packet("5522;type=read:status=DONE").expect("done");
        assert!(matches!(assembler.push(&done), Ok(AssembleOutcome::Complete(_))));
        // Packet after DONE.
        assert_eq!(assembler.push(&done), Err(AssembleError::OutOfOrder));
    }

    #[test]
    fn assembler_rejects_id_mismatch_but_tolerates_absent_id() {
        let mut assembler = ReadAssembler::new("id-1");
        let foreign = parse_read_packet("5522;type=read:status=OK:id=id-2").expect("foreign ok");
        assert_eq!(assembler.push(&foreign), Err(AssembleError::IdMismatch));

        let anonymous = parse_read_packet("5522;type=read:status=OK").expect("anonymous ok");
        assert_eq!(assembler.push(&anonymous), Ok(AssembleOutcome::NeedMore));
    }

    #[test]
    fn assembler_maps_terminal_errors_and_invalid_chunks() {
        let mut assembler = ReadAssembler::new("id-1");
        let ok = parse_read_packet("5522;type=read:status=OK").expect("ok");
        assert_eq!(assembler.push(&ok), Ok(AssembleOutcome::NeedMore));

        let busy = parse_read_packet("5522;type=read:status=EBUSY").expect("busy");
        assert_eq!(assembler.push(&busy), Err(AssembleError::Terminal(CompactString::const_new("busy"))));

        let mut assembler = ReadAssembler::new("id-1");
        assert_eq!(assembler.push(&ok), Ok(AssembleOutcome::NeedMore));
        let mut bad = data_packet("YQ==", "YWI=");
        bad.data_b64 = "YW I=".to_string();
        assert_eq!(assembler.push(&bad), Err(AssembleError::InvalidBase64));
    }

    #[test]
    fn assembler_enforces_decoded_size_cap() {
        let mut assembler = ReadAssembler::with_max_bytes("id-1", 3);
        let ok = parse_read_packet("5522;type=read:status=OK").expect("ok");
        assert_eq!(assembler.push(&ok), Ok(AssembleOutcome::NeedMore));
        // "YWI=" decodes to 2 bytes: fits; the second chunk overflows.
        assert_eq!(assembler.push(&data_packet("YQ==", "YWI=")), Ok(AssembleOutcome::NeedMore));
        assert_eq!(assembler.push(&data_packet("YQ==", "YWI=")), Err(AssembleError::TooLarge));
    }

    #[test]
    fn hint_roundtrips_and_defaults_to_unset() {
        set_clipboard_5522_hint(None);
        assert_eq!(clipboard_5522_hint(), None);
        set_clipboard_5522_hint(Some(true));
        assert_eq!(clipboard_5522_hint(), Some(true));
        set_clipboard_5522_hint(Some(false));
        assert_eq!(clipboard_5522_hint(), Some(false));
        set_clipboard_5522_hint(None);
    }

    #[test]
    fn available_resolves_env_over_hint_with_fail_closed_default() {
        let guard = crate::env_lock::lock();
        let previous = std::env::var_os("VTCODE_CLIPBOARD_5522");
        set_clipboard_5522_hint(None);

        guard.remove_var("VTCODE_CLIPBOARD_5522");
        assert!(!clipboard_5522_available());

        set_clipboard_5522_hint(Some(true));
        assert!(clipboard_5522_available());
        guard.set_var("VTCODE_CLIPBOARD_5522", "off");
        assert!(!clipboard_5522_available());

        set_clipboard_5522_hint(Some(false));
        guard.set_var("VTCODE_CLIPBOARD_5522", "yes");
        assert!(clipboard_5522_available());

        guard.restore_var("VTCODE_CLIPBOARD_5522", previous);
        set_clipboard_5522_hint(None);
    }
}
