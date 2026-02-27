use std::path::Path;

use anyhow::{Context, Result};

pub fn read_file_with_context_sync(path: &Path, purpose: &str) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {} at {}", purpose, path.display()))
}

pub fn write_file_with_context_sync(path: &Path, content: &str, purpose: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for {} at {}",
                purpose,
                parent.display()
            )
        })?;
    }

    std::fs::write(path, content)
        .with_context(|| format!("Failed to write {} at {}", purpose, path.display()))
}

pub fn is_image_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tiff" | "tif" | "svg"
    )
}
