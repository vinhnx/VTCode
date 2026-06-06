//! File operation tools with composable functionality.
//!
//! This module provides the `FileOpsTool` for file discovery and listing operations,
//! along with supporting utilities for diff previews and path helpers.

mod diff_preview;
mod list;
mod path_policy;
mod read;
mod tool;
mod write;

pub use diff_preview::{
    build_diff_preview, diff_preview_error_skip, diff_preview_size_skip, diff_preview_suppressed,
};
pub use tool::FileOpsTool;
pub use vtcode_commons::fs::is_image_path;

/// Restore exact text content by accounting for trailing newline differences.
///
/// When a file's byte size is slightly larger than the content string length,
/// this reconstructs the original content by appending the missing trailing
/// newline(s). Returns `None` if the size mismatch is unexpected.
pub(crate) fn restore_exact_text_content(content: &str, size_bytes: u64) -> Option<String> {
    let content_size = content.len() as u64;
    match size_bytes.checked_sub(content_size) {
        Some(0) => Some(content.to_string()),
        Some(1) => Some(format!("{content}\n")),
        Some(2) if content.contains("\r\n") || !content.contains('\n') => {
            Some(format!("{content}\r\n"))
        }
        _ => None,
    }
}
