use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default)]
struct RuntimeDebugContext {
    debug_session_id: Option<String>,
    archive_session_id: Option<String>,
    debug_log_path: Option<PathBuf>,
}

static RUNTIME_DEBUG_CONTEXT: LazyLock<Mutex<RuntimeDebugContext>> =
    LazyLock::new(|| Mutex::new(RuntimeDebugContext::default()));

fn with_runtime_debug_context<R>(f: impl FnOnce(&mut RuntimeDebugContext) -> R) -> R {
    match RUNTIME_DEBUG_CONTEXT.lock() {
        Ok(mut context) => f(&mut context),
        Err(poisoned) => {
            let mut context = poisoned.into_inner();
            f(&mut context)
        }
    }
}

pub(crate) fn configure_runtime_debug_context(
    debug_session_id: String,
    archive_session_id: Option<String>,
) {
    with_runtime_debug_context(|context| {
        context.debug_session_id = Some(debug_session_id);
        context.archive_session_id = archive_session_id;
        context.debug_log_path = None;
    });
}

pub(crate) fn runtime_archive_session_id() -> Option<String> {
    with_runtime_debug_context(|context| context.archive_session_id.clone())
}

pub(crate) fn runtime_debug_log_path() -> Option<PathBuf> {
    with_runtime_debug_context(|context| context.debug_log_path.clone())
}

pub(super) fn set_runtime_debug_log_path(path: PathBuf) {
    with_runtime_debug_context(|context| {
        context.debug_log_path = Some(path);
    });
}

pub(super) fn sanitize_debug_component(value: &str, fallback: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if matches!(ch, '-' | '_') {
            if !last_was_separator {
                normalized.push(ch);
                last_was_separator = true;
            }
        } else if !last_was_separator {
            normalized.push('-');
            last_was_separator = true;
        }
    }

    let trimmed = normalized.trim_matches(|c| c == '-' || c == '_');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn build_command_debug_session_id(mode_hint: &str) -> String {
    let mode = sanitize_debug_component(mode_hint, "cmd");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("cmd-{mode}-{timestamp}-{}", std::process::id())
}

pub(super) fn current_debug_session_id() -> String {
    with_runtime_debug_context(|context| context.debug_session_id.clone())
        .unwrap_or_else(|| build_command_debug_session_id("default"))
}
