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
