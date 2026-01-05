//! File operation tools with composable functionality.
//!
//! This module provides the `FileOpsTool` for file discovery and listing operations,
//! along with supporting utilities for diff previews and path helpers.

mod diff_preview;
mod tool;

pub use diff_preview::{
    build_diff_preview, diff_preview_error_skip, diff_preview_size_skip, diff_preview_suppressed,
};
pub use tool::FileOpsTool;

use std::path::Path;

/// Check if a file path represents an image based on its extension.
pub fn is_image_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    let lowercase = extension.to_ascii_lowercase();
    matches!(
        lowercase.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tiff" | "tif" | "svg"
    )
}
