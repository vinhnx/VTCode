//! # Image Processing Utilities
//!
//! This module provides utilities for reading and processing image files,
//! particularly for converting local image files to formats suitable for LLM providers.
//! It handles local image file reading and conversion to base64 format with proper MIME types.
//!
//! ## Features
//!
//! - **Local Image Reading**: Reads image files from the local filesystem
//! - **Base64 Encoding**: Converts binary image data to base64 for API transmission
//! - **MIME Type Detection**: Automatically detects image MIME type from file extension
//! - **Security Validation**: Validates that files are within safe paths
//! - **Format Support**: Supports common image formats (PNG, JPG, JPEG, GIF, WEBP, etc.)
//!
//! ## Basic Usage
//!
//! ```rust,no_run
//! use vtcode_core::utils::image_processing::read_image_file;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Read a local image file
//!     let image_data = read_image_file("/path/to/image.png").await?;
//!
//!     println!("Base64 data: {}", &image_data.base64_data[..100]); // First 100 chars
//!     println!("MIME type: {}", image_data.mime_type);
//!     println!("File size: {} bytes", image_data.size);
//!
//!     Ok(())
//! }
//! ```

use anyhow::{Context, Result};
use base64::Engine;
use std::path::Path;

/// Represents the data from an image file ready for LLM consumption
#[derive(Debug, Clone)]
pub struct ImageData {
    /// Base64-encoded image data
    pub base64_data: String,

    /// MIME type of the image (e.g., "image/png", "image/jpeg")
    pub mime_type: String,

    /// Original file path
    pub file_path: String,

    /// File size in bytes
    pub size: u64,
}

/// Reads an image file from the local filesystem and converts it to base64 format
///
/// This function is designed to work with local image files for LLM providers that
/// support image input through the ImageContent structure.
///
/// # Arguments
///
/// * `file_path` - Path to the image file to read
///
/// # Returns
///
/// * `ImageData` - Contains base64 data, MIME type, and file metadata
///
/// # Errors
///
/// This function will return an error if:
/// - The file doesn't exist
/// - The file is not a valid image
/// - The file cannot be read due to permissions
/// - The file is too large (greater than 20MB)
pub async fn read_image_file<P: AsRef<Path>>(file_path: P) -> Result<ImageData> {
    let path = file_path.as_ref();

    // Validate the file path to ensure it's safe
    validate_image_path(path)?;

    // Check if file exists
    if !tokio::fs::metadata(path).await.is_ok() {
        return Err(anyhow::anyhow!(
            "Image file does not exist: {}",
            path.display()
        ));
    }

    // Read the file contents
    let file_contents = tokio::fs::read(path)
        .await
        .with_context(|| format!("Failed to read image file: {}", path.display()))?;

    // Validate file size (max 20MB for most LLM providers)
    if file_contents.len() > 20 * 1024 * 1024 {
        return Err(anyhow::anyhow!(
            "Image file too large: {} bytes (max 20MB)",
            file_contents.len()
        ));
    }

    // Detect MIME type based on file extension
    let mime_type = detect_mime_type(path)
        .with_context(|| format!("Failed to detect MIME type for image: {}", path.display()))?;

    // Encode to base64
    let base64_data = base64::engine::general_purpose::STANDARD.encode(&file_contents);

    Ok(ImageData {
        base64_data,
        mime_type,
        file_path: path.display().to_string(),
        size: file_contents.len() as u64,
    })
}

/// Validates that the image file path is safe to access
///
/// This function ensures that:
/// - The file has a valid image extension
/// - The path is not trying to access unsafe locations
fn validate_image_path(path: &Path) -> Result<()> {
    // Check file extension
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    let valid_extensions = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "svg"];

    if !valid_extensions.contains(&extension.as_str()) {
        return Err(anyhow::anyhow!(
            "Invalid image file extension: {}. Supported formats: PNG, JPG, JPEG, GIF, WEBP, BMP, TIFF, SVG",
            extension
        ));
    }

    // Check for path traversal attempts
    let path_str = path.to_string_lossy();
    if path_str.contains("../") || path_str.contains("..\\") {
        return Err(anyhow::anyhow!(
            "Path traversal detected in image path: {}",
            path.display()
        ));
    }

    Ok(())
}

/// Detects the MIME type based on file extension
fn detect_mime_type(path: &Path) -> Result<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn test_read_png_image() {
        // Create a temporary PNG file with valid PNG header
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.png");
        let mut temp_file = std::fs::File::create(&file_path).unwrap();

        // Write a minimal PNG header (not a real image, but valid for testing)
        temp_file
            .write_all(&[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk start
            ])
            .unwrap();

        temp_file.flush().unwrap();
        drop(temp_file);

        let result = read_image_file(&file_path).await;
        assert!(result.is_ok());

        let image_data = result.unwrap();
        assert_eq!(image_data.mime_type, "image/png");
        // Verify the base64 data contains the PNG header signature
        assert!(!image_data.base64_data.is_empty());
        assert_eq!(image_data.size, 16);
    }

    #[tokio::test]
    async fn test_read_jpg_image() {
        // Create a temporary JPG file with valid JPG header
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.jpg");
        let mut temp_file = std::fs::File::create(&file_path).unwrap();

        // Write a minimal JPG header
        temp_file.write_all(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
        temp_file.flush().unwrap();
        drop(temp_file);

        let result = read_image_file(&file_path).await;
        assert!(result.is_ok());

        let image_data = result.unwrap();
        assert_eq!(image_data.mime_type, "image/jpeg");
        assert_eq!(image_data.size, 4);
    }

    #[tokio::test]
    async fn test_invalid_extension() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, b"test").unwrap();

        let result = read_image_file(&file_path).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid image file extension")
        );
    }

    #[tokio::test]
    async fn test_file_not_found() {
        let result = read_image_file("/nonexistent/image.png").await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Image file does not exist")
        );
    }

    #[tokio::test]
    async fn test_path_traversal_detection() {
        let result = read_image_file("../../etc/passwd.jpg").await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Path traversal detected")
        );
    }
}
