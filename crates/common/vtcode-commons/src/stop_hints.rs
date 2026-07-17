pub const STOP_HINT_COMPACT: &str = "Esc stop • Ctrl+C stop • /stop";
pub const STOP_HINT_INLINE: &str = "Esc, Ctrl+C, or /stop to stop";

pub fn with_stop_hint(message: &str) -> String {
    if message.trim().is_empty() {
        STOP_HINT_INLINE.to_string()
    } else {
        format!("{message} ({STOP_HINT_INLINE})")
    }
}
