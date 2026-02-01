//! Image processing utilities

use anyhow::Result;
use base64::Engine;
use std::path::Path;

/// Represents the data from an image file ready for LLM consumption
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImageData {
    /// Base64-encoded image data
    pub base64_data: String,

    /// MIME type of the image (e.g., "image/png", "image/jpeg")
    pub mime_type: String,

    /// Original file path or URL
    pub file_path: String,

    /// File size in bytes
    pub size: u64,
}

/// Detects MIME type from Content-Type header
pub fn detect_mime_type_from_content_type(content_type: &str) -> Option<String> {
    let content_type = content_type.to_lowercase();
    if content_type.starts_with("image/png") {
        Some("image/png".to_string())
    } else if content_type.starts_with("image/jpeg") || content_type.starts_with("image/jpg") {
        Some("image/jpeg".to_string())
    } else if content_type.starts_with("image/gif") {
        Some("image/gif".to_string())
    } else if content_type.starts_with("image/webp") {
        Some("image/webp".to_string())
    } else if content_type.starts_with("image/bmp") {
        Some("image/bmp".to_string())
    } else if content_type.starts_with("image/tiff") || content_type.starts_with("image/tif") {
        Some("image/tiff".to_string())
    } else if content_type.starts_with("image/svg") {
        Some("image/svg+xml".to_string())
    } else {
        None
    }
}

/// Detects MIME type from file data (magic bytes)
pub fn detect_mime_type_from_data(data: &[u8]) -> String {
    // JPEG magic bytes: starts with FF D8
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
        return "image/jpeg".to_string();
    }

    // Need at least 8 bytes for other formats
    if data.len() < 8 {
        return "image/png".to_string();
    }

    match &data[..8] {
        [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] => "image/png".to_string(),
        [0x47, 0x49, 0x46, 0x38, _, _, _, _] => {
            if data.len() >= 12 && &data[8..12] == b"WEBP" {
                "image/webp".to_string()
            } else {
                "image/gif".to_string()
            }
        }
        [0x52, 0x49, 0x46, 0x46, _, _, _, _] => {
            if data.len() >= 12 && &data[8..12] == b"WEBP" {
                "image/webp".to_string()
            } else {
                "image/png".to_string()
            }
        }
        [0x42, 0x4D, _, _] => "image/bmp".to_string(),
        _ => "image/png".to_string(),
    }
}

/// Detects the MIME type based on file extension
pub fn detect_mime_type_from_extension(path: &Path) -> Result<String> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    let mime_type = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        "svg" => "image/svg+xml",
        _ => return Err(anyhow::anyhow!("Unsupported image format: {}", extension)),
    };

    Ok(mime_type.to_string())
}

/// Validates that the image file path has a supported extension
pub fn has_supported_image_extension(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    const VALID_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "svg"];
    VALID_EXTENSIONS.contains(&extension.as_str())
}

/// Encodes binary data to base64
pub fn encode_to_base64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}
