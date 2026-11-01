//! Cross-process session mailbox: peer Q&A between concurrent CLI sessions.
//!
//! Layout inside a session directory:
//!
//! - `mailbox/<message-id>.json` — unread messages, one atomic file each, so
//!   concurrent senders never interleave.
//! - `mailbox/read/<message-id>.json` — acknowledged or expired messages,
//!   kept briefly for audit and reaped by age on every drain.
//!
//! Delivery is at-least-once: the receiver drains, injects into its turn,
//! then acknowledges. A crash between inject and ack redelivers on the next
//! drain. Only live sessions (see `retention::session_dir_is_live`) accept
//! mail; sending to an unknown or dead session fails closed instead of
//! piling up questions nobody will read.

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::SessionStoreError;

/// Schema version for mailbox envelopes.
pub const MAILBOX_SCHEMA_VERSION: u32 = 1;
/// Inbox directory inside a session directory.
pub const MAILBOX_DIR: &str = "mailbox";
/// Acknowledged/expired archive inside the inbox directory.
const READ_SUBDIR: &str = "read";

/// Maximum serialized envelope size in bytes.
pub const MAX_MAIL_BYTES: usize = 32_768;
/// Maximum body/answer text in characters.
pub const MAX_TEXT_CHARS: usize = 8_000;
/// Maximum structured questions per question message (mirrors `request_user_input`).
pub const MAX_QUESTIONS: usize = 3;
/// Maximum question text in characters.
pub const MAX_QUESTION_CHARS: usize = 2_000;
/// Maximum options per question (mirrors `request_user_input`).
pub const MAX_OPTIONS: usize = 3;
/// Maximum option label in characters.
pub const MAX_OPTION_LABEL_CHARS: usize = 80;
/// Maximum option description in characters.
pub const MAX_OPTION_DESCRIPTION_CHARS: usize = 240;
/// Maximum question id in characters.
pub const MAX_QUESTION_ID_CHARS: usize = 64;
/// Maximum unread messages per inbox; sends beyond this fail closed.
pub const MAX_INBOX_DEPTH: usize = 100;
/// Default message time-to-live in seconds.
pub const DEFAULT_TTL_SECS: i64 = 3_600;
/// Minimum message time-to-live in seconds.
pub const MIN_TTL_SECS: i64 = 60;
/// Maximum message time-to-live in seconds.
pub const MAX_TTL_SECS: i64 = 86_400;
/// How long acknowledged/expired messages are kept for audit, in seconds.
pub const READ_RETENTION_SECS: i64 = 7 * 86_400;

/// Kind of a mailbox message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailKind {
    /// One or more questions for the peer session (async; the answer arrives
    /// as a separate message referencing this one).
    Question,
    /// Answer to a question message; carries `in_reply_to`.
    Answer,
    /// Fire-and-forget note; no reply expected.
    Notice,
}

/// One suggested choice for a structured question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailOption {
    /// User-facing label (1-5 words).
    pub label: String,
    /// One short sentence explaining impact/tradeoff if selected.
    pub description: String,
}

/// One structured question inside a question message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailQuestion {
    /// Stable id for mapping answers (opaque to the transport).
    pub id: String,
    /// Single-sentence prompt for the peer session.
    pub question: String,
    /// Optional 2-3 mutually exclusive choices, recommended first.
    #[serde(default)]
    pub options: Vec<MailOption>,
}

/// A validated, sendable mailbox message under construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMail {
    to_session: String,
    kind: MailKind,
    in_reply_to: Option<String>,
    questions: Vec<MailQuestion>,
    text: String,
    ttl_secs: i64,
}

impl NewMail {
    /// A question message: 1–3 structured questions plus an optional freeform
    /// body. Fails closed on any bound violation.
    pub fn question(
        to_session: &str,
        questions: Vec<MailQuestion>,
        body: &str,
        ttl_secs: Option<i64>,
    ) -> Result<Self, SessionStoreError> {
        validate_session_id(to_session)?;
        if questions.is_empty() || questions.len() > MAX_QUESTIONS {
            return Err(SessionStoreError::invalid_mail(format!(
                "question messages carry 1-{MAX_QUESTIONS} questions, got {}",
                questions.len()
            )));
        }
        for question in &questions {
            validate_question(question)?;
        }
        validate_text_len("body", body, MAX_TEXT_CHARS)?;
        Ok(Self {
            to_session: to_session.to_owned(),
            kind: MailKind::Question,
            in_reply_to: None,
            questions,
            text: body.to_owned(),
            ttl_secs: clamp_ttl(ttl_secs),
        })
    }

    /// An answer message: freeform text referencing the question message id.
    pub fn answer(
        to_session: &str,
        in_reply_to: &str,
        text: &str,
        ttl_secs: Option<i64>,
    ) -> Result<Self, SessionStoreError> {
        validate_session_id(to_session)?;
        if in_reply_to.is_empty() || in_reply_to.len() > 128 {
            return Err(SessionStoreError::invalid_mail("answer requires the question message id"));
        }
        validate_text("answer text", text)?;
        Ok(Self {
            to_session: to_session.to_owned(),
            kind: MailKind::Answer,
            in_reply_to: Some(in_reply_to.to_owned()),
            questions: Vec::new(),
            text: text.to_owned(),
            ttl_secs: clamp_ttl(ttl_secs),
        })
    }

    /// A fire-and-forget notice.
    pub fn notice(to_session: &str, text: &str, ttl_secs: Option<i64>) -> Result<Self, SessionStoreError> {
        validate_session_id(to_session)?;
        validate_text("notice text", text)?;
        Ok(Self {
            to_session: to_session.to_owned(),
            kind: MailKind::Notice,
            in_reply_to: None,
            questions: Vec::new(),
            text: text.to_owned(),
            ttl_secs: clamp_ttl(ttl_secs),
        })
    }
}

/// A persisted mailbox message as read from disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailMessage {
    /// Envelope schema version ([`MAILBOX_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// Sender-generated unique id (also the filename stem).
    pub message_id: String,
    /// Session that sent the message.
    pub from_session: String,
    /// Session that owns the inbox.
    pub to_session: String,
    /// What the receiver should do with the message.
    pub kind: MailKind,
    /// Question message id this answers (answers only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<String>,
    /// Structured questions (question messages only).
    #[serde(default)]
    pub questions: Vec<MailQuestion>,
    /// Freeform body: context for questions, content for answers/notices.
    pub text: String,
    /// RFC3339 creation timestamp.
    pub created_at: String,
    /// RFC3339 expiry timestamp.
    pub expires_at: String,
}

/// Receipt for an accepted message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailReceipt {
    /// The accepted message's id, for correlating answers.
    pub message_id: String,
}

/// Result of draining an inbox: live messages to inject, ids of expired
/// messages moved aside, and a count of quarantined malformed files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxDrain {
    /// Unexpired messages, oldest first.
    pub live: Vec<MailMessage>,
    /// Ids of expired messages moved to the read archive.
    pub expired_ids: Vec<String>,
    /// Count of malformed files quarantined to the read archive.
    pub quarantined: u64,
}

fn validate_session_id(session: &str) -> Result<(), SessionStoreError> {
    // The store sanitizes ids into directory names; addressing must use the
    // canonical form so two distinct ids can never resolve to one inbox.
    let canonical = !session.is_empty()
        && session.len() <= 80
        && !session.starts_with('.')
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.');
    if canonical {
        Ok(())
    } else {
        Err(SessionStoreError::invalid_mail(format!("invalid session id: {session:?}")))
    }
}

fn validate_text_len(field: &str, text: &str, max_chars: usize) -> Result<(), SessionStoreError> {
    if text.chars().count() > max_chars {
        return Err(SessionStoreError::invalid_mail(format!("{field} exceeds {max_chars} characters")));
    }
    Ok(())
}

fn validate_text(field: &str, text: &str) -> Result<(), SessionStoreError> {
    if text.trim().is_empty() {
        return Err(SessionStoreError::invalid_mail(format!("{field} must not be empty")));
    }
    validate_text_len(field, text, MAX_TEXT_CHARS)
}

fn validate_question(question: &MailQuestion) -> Result<(), SessionStoreError> {
    if question.id.is_empty() || question.id.len() > MAX_QUESTION_ID_CHARS {
        return Err(SessionStoreError::invalid_mail("question id must be 1-64 characters"));
    }
    if question.question.trim().is_empty() {
        return Err(SessionStoreError::invalid_mail("question text must not be empty"));
    }
    validate_text_len("question text", &question.question, MAX_QUESTION_CHARS)?;
    if question.options.len() > MAX_OPTIONS {
        return Err(SessionStoreError::invalid_mail(format!("questions carry at most {MAX_OPTIONS} options")));
    }
    for option in &question.options {
        if option.label.trim().is_empty() {
            return Err(SessionStoreError::invalid_mail("option label must not be empty"));
        }
        validate_text_len("option label", &option.label, MAX_OPTION_LABEL_CHARS)?;
        if option.description.trim().is_empty() {
            return Err(SessionStoreError::invalid_mail("option description must not be empty"));
        }
        validate_text_len("option description", &option.description, MAX_OPTION_DESCRIPTION_CHARS)?;
    }
    Ok(())
}

fn clamp_ttl(ttl_secs: Option<i64>) -> i64 {
    ttl_secs.unwrap_or(DEFAULT_TTL_SECS).clamp(MIN_TTL_SECS, MAX_TTL_SECS)
}

fn inbox_dir(workspace: &Path, session: &str) -> PathBuf {
    crate::session_dir(workspace, session).join(MAILBOX_DIR)
}

fn read_dir_of(inbox: &Path) -> PathBuf {
    inbox.join(READ_SUBDIR)
}

fn unread_count(inbox: &Path) -> Result<usize, SessionStoreError> {
    let entries = std::fs::read_dir(inbox).map_err(|error| SessionStoreError::io(inbox.to_path_buf(), error))?;
    let mut count = 0;
    for entry in entries.flatten() {
        // Symlinks are foreign objects (see `drain_inbox`): never let a
        // planted link fake a full inbox.
        if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
            continue;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_message = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".json"));
        if is_message {
            count += 1;
        }
    }
    Ok(count)
}

/// Send a validated message to a live session's inbox.
///
/// Fails closed when the target session is unknown or not live, when its
/// inbox is full, or when the envelope exceeds [`MAX_MAIL_BYTES`].
pub fn send_mail(workspace: &Path, from_session: &str, mail: &NewMail) -> Result<MailReceipt, SessionStoreError> {
    validate_session_id(from_session)?;
    let target_dir = crate::session_dir(workspace, &mail.to_session);
    if !target_dir.is_dir() {
        return Err(SessionStoreError::session_not_live(&mail.to_session));
    }
    if !crate::retention::session_dir_is_live(&target_dir) {
        return Err(SessionStoreError::session_not_live(&mail.to_session));
    }
    let inbox = inbox_dir(workspace, &mail.to_session);
    crate::ensure_private_directory(&inbox)?;
    if unread_count(&inbox)? >= MAX_INBOX_DEPTH {
        return Err(SessionStoreError::mailbox_full(&mail.to_session));
    }

    let created = Utc::now();
    let expires = created + chrono::Duration::seconds(mail.ttl_secs);
    // Claim an unused id; a collision regenerates instead of overwriting.
    for _ in 0..3 {
        let message_id = uuid::Uuid::new_v4().to_string();
        let message = MailMessage {
            schema_version: MAILBOX_SCHEMA_VERSION,
            message_id: message_id.clone(),
            from_session: from_session.to_owned(),
            to_session: mail.to_session.clone(),
            kind: mail.kind,
            in_reply_to: mail.in_reply_to.clone(),
            questions: mail.questions.clone(),
            text: mail.text.clone(),
            created_at: created.to_rfc3339(),
            expires_at: expires.to_rfc3339(),
        };
        let bytes = serde_json::to_vec(&message)?;
        if bytes.len() > MAX_MAIL_BYTES {
            return Err(SessionStoreError::invalid_mail(format!("envelope exceeds {MAX_MAIL_BYTES} bytes")));
        }
        let path = inbox.join(format!("{message_id}.json"));
        let claimed = vtcode_commons::VtCodePaths::write_private_file_atomic_if_absent(&path, &bytes)
            .map_err(|error| SessionStoreError::io(path.clone(), std::io::Error::other(error)))?;
        if claimed {
            return Ok(MailReceipt { message_id });
        }
    }
    Err(SessionStoreError::invalid_mail("message id collision; retry the send"))
}

/// Parse one inbox file into a message, verifying the envelope matches its filename.
fn read_message_file(path: &Path) -> Result<MailMessage, SessionStoreError> {
    // Bounded read: the send path caps envelopes at MAX_MAIL_BYTES, but a
    // planted file bypasses the send path — never read it unbounded.
    use std::io::Read as _;
    let file = std::fs::File::open(path).map_err(|error| SessionStoreError::io(path.to_path_buf(), error))?;
    let mut bytes = Vec::new();
    file.take((MAX_MAIL_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| SessionStoreError::io(path.to_path_buf(), error))?;
    if bytes.len() > MAX_MAIL_BYTES {
        return Err(SessionStoreError::invalid_mail(format!("envelope exceeds {MAX_MAIL_BYTES} bytes")));
    }
    let message: MailMessage = serde_json::from_slice(&bytes).map_err(SessionStoreError::from)?;
    if message.schema_version != MAILBOX_SCHEMA_VERSION {
        return Err(SessionStoreError::invalid_mail(format!("unsupported mailbox schema {}", message.schema_version)));
    }
    let stem_matches = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == message.message_id);
    if !stem_matches {
        return Err(SessionStoreError::invalid_mail("message id does not match filename"));
    }
    Ok(message)
}

fn message_is_expired(message: &MailMessage, now: &chrono::DateTime<Utc>) -> bool {
    let Ok(expires) = chrono::DateTime::parse_from_rfc3339(&message.expires_at) else {
        return true;
    };
    expires <= *now
}

fn move_to_read(inbox: &Path, file_name: &str) -> Result<(), SessionStoreError> {
    let read_dir = read_dir_of(inbox);
    crate::ensure_private_directory(&read_dir)?;
    let from = inbox.join(file_name);
    let to = read_dir.join(file_name);
    // `to` exists only when a foreign file was planted under a claimed name
    // (claims are unique, so our own moves never collide): keep the
    // first-archived copy for audit and drop this one. Both operations act on
    // the link itself, never through it.
    if to.exists() {
        std::fs::remove_file(&from).map_err(|error| SessionStoreError::io(from.clone(), error))?;
        return Ok(());
    }
    std::fs::rename(&from, &to).map_err(|error| SessionStoreError::io(from, error))?;
    Ok(())
}

/// Reap read messages older than [`READ_RETENTION_SECS`]. Best-effort:
/// unstattable files are kept for the next round.
fn reap_read_dir(inbox: &Path, now: &chrono::DateTime<Utc>) -> Result<(), SessionStoreError> {
    let read_dir = read_dir_of(inbox);
    let entries = match std::fs::read_dir(&read_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(SessionStoreError::io(read_dir, error)),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let modified = path.metadata().and_then(|meta| meta.modified()).ok().and_then(|modified| {
            chrono::DateTime::<Utc>::from_timestamp(
                modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64,
                0,
            )
        });
        let stale = modified.is_some_and(|mtime| (*now - mtime).num_seconds() > READ_RETENTION_SECS);
        if stale
            && let Err(error) = std::fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(SessionStoreError::io(path, error));
        }
    }
    Ok(())
}

/// Drain an inbox: return live messages oldest-first, move expired and
/// malformed files to the read archive, and reap aged read messages.
///
/// Callers inject `live` into the turn and then [`ack_mail`] each delivered
/// message; unacked messages redeliver on the next drain (at-least-once).
pub fn drain_inbox(workspace: &Path, session: &str) -> Result<InboxDrain, SessionStoreError> {
    validate_session_id(session)?;
    let inbox = inbox_dir(workspace, session);
    if !inbox.is_dir() {
        return Ok(InboxDrain::default());
    }
    let now = Utc::now();
    let mut drain = InboxDrain::default();
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&inbox).map_err(|error| SessionStoreError::io(inbox.clone(), error))? {
        let entry = entry.map_err(|error| SessionStoreError::io(inbox.clone(), std::io::Error::other(error)))?;
        let path = entry.path();
        // Symlinks are foreign objects (`send_mail` only writes regular
        // files): quarantine the link itself instead of following it, so a
        // planted link can neither inject outside content nor loop the drain.
        if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                move_to_read(&inbox, name)?;
                drain.quarantined += 1;
            }
            continue;
        }
        if !path.is_file() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            if name.ends_with(".json") {
                names.push(name.to_owned());
            }
        }
    }
    names.sort();
    for name in names {
        let path = inbox.join(&name);
        let message = match read_message_file(&path) {
            Ok(message) => message,
            Err(_) => {
                move_to_read(&inbox, &name)?;
                drain.quarantined += 1;
                continue;
            }
        };
        if message_is_expired(&message, &now) {
            move_to_read(&inbox, &name)?;
            drain.expired_ids.push(message.message_id);
        } else {
            drain.live.push(message);
        }
    }
    drain
        .live
        .sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.message_id.cmp(&b.message_id)));
    reap_read_dir(&inbox, &now)?;
    Ok(drain)
}

/// Acknowledge a delivered message, moving it to the read archive.
/// Returns `false` when the message was already gone (double-ack is safe).
pub fn ack_mail(workspace: &Path, session: &str, message_id: &str) -> Result<bool, SessionStoreError> {
    validate_session_id(session)?;
    if uuid::Uuid::parse_str(message_id).is_err() {
        return Err(SessionStoreError::invalid_mail("invalid message id"));
    }
    let inbox = inbox_dir(workspace, session);
    let path = inbox.join(format!("{message_id}.json"));
    if !path.is_file() {
        return Ok(false);
    }
    let name = format!("{message_id}.json");
    move_to_read(&inbox, &name)?;
    Ok(true)
}

/// Fetch one unread message by id without acknowledging it.
pub fn read_mail(workspace: &Path, session: &str, message_id: &str) -> Result<Option<MailMessage>, SessionStoreError> {
    validate_session_id(session)?;
    if uuid::Uuid::parse_str(message_id).is_err() {
        return Err(SessionStoreError::invalid_mail("invalid message id"));
    }
    let path = inbox_dir(workspace, session).join(format!("{message_id}.json"));
    if !path.is_file() {
        return Ok(None);
    }
    read_message_file(&path).map(Some)
}

/// Find a message by id in the live inbox or the read archive.
///
/// Answers arrive turns after their question was drained and acked, so reply
/// resolution must look past the live inbox into the archive.
pub fn find_mail(workspace: &Path, session: &str, message_id: &str) -> Result<Option<MailMessage>, SessionStoreError> {
    if let Some(message) = read_mail(workspace, session, message_id)? {
        return Ok(Some(message));
    }
    let path = read_dir_of(&inbox_dir(workspace, session)).join(format!("{message_id}.json"));
    if !path.is_file() {
        return Ok(None);
    }
    read_message_file(&path).map(Some)
}
