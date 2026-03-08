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

use std::path::Path;

pub fn is_image_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        extension,
        _ if extension.eq_ignore_ascii_case("png")
            || extension.eq_ignore_ascii_case("jpg")
            || extension.eq_ignore_ascii_case("jpeg")
            || extension.eq_ignore_ascii_case("gif")
            || extension.eq_ignore_ascii_case("bmp")
            || extension.eq_ignore_ascii_case("webp")
            || extension.eq_ignore_ascii_case("tiff")
            || extension.eq_ignore_ascii_case("tif")
            || extension.eq_ignore_ascii_case("svg")
    )
}
