//! Image processing utilities
//!
//! Re-exports and high-level wrappers for image processing.

use anyhow::{Context, Result};
use std::path::Path;
pub use vtcode_commons::image::*;
use vtcode_commons::paths::is_safe_relative_path;

/// Reads an image from a URL and converts it to base64 format
pub async fn read_image_from_url(url: &str) -> Result<ImageData> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(anyhow::anyhow!("Invalid URL: {}", url));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch image from URL: {}", url))?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "HTTP error when fetching image: {} (status: {})",
            url,
            response.status()
        ));
    }

    let content_length = response.content_length().unwrap_or(0);
    if content_length > 20 * 1024 * 1024 {
        return Err(anyhow::anyhow!(
            "Image from URL too large: {} bytes (max 20MB)",
            content_length
        ));
    }

    let mime_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(detect_mime_type_from_content_type);

    let file_contents = response
        .bytes()
        .await
        .with_context(|| format!("Failed to read response body from: {}", url))?
        .to_vec();

    let mime_type = mime_type.unwrap_or_else(|| detect_mime_type_from_data(&file_contents));
    let base64_data = encode_to_base64(&file_contents);

    Ok(ImageData {
        base64_data,
        mime_type,
        file_path: url.to_string(),
        size: file_contents.len() as u64,
    })
}

/// Reads an image file from the local filesystem and converts it to base64 format
pub async fn read_image_file<P: AsRef<Path>>(file_path: P) -> Result<ImageData> {
    let path = file_path.as_ref();

    // Validate the file path to ensure it's safe
    if !is_safe_relative_path(&path.to_string_lossy()) {
        return Err(anyhow::anyhow!(
            "Unsafe or traversal detected in image path: {}",
            path.display()
        ));
    }

    if !has_supported_image_extension(path) {
        return Err(anyhow::anyhow!(
            "Unsupported image extension for path: {}",
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
    let mime_type = detect_mime_type_from_extension(path)?;

    // Encode to base64
    let base64_data = encode_to_base64(&file_contents);

    Ok(ImageData {
        base64_data,
        mime_type,
        file_path: path.display().to_string(),
        size: file_contents.len() as u64,
    })
}

/// Reads an image file from an absolute path (or already validated path) and converts it to base64.
///
/// This skips relative-path safety checks and should only be used when the caller has validated
/// the path scope and intent.
pub async fn read_image_file_any_path<P: AsRef<Path>>(file_path: P) -> Result<ImageData> {
    let path = file_path.as_ref();

    if !has_supported_image_extension(path) {
        return Err(anyhow::anyhow!(
            "Unsupported image extension for path: {}",
            path.display()
        ));
    }

    let file_contents = tokio::fs::read(path)
        .await
        .with_context(|| format!("Failed to read image file: {}", path.display()))?;

    if file_contents.len() > 20 * 1024 * 1024 {
        return Err(anyhow::anyhow!(
            "Image file too large: {} bytes (max 20MB)",
            file_contents.len()
        ));
    }

    let mime_type = detect_mime_type_from_extension(path)?;
    let base64_data = encode_to_base64(&file_contents);

    Ok(ImageData {
        base64_data,
        mime_type,
        file_path: path.display().to_string(),
        size: file_contents.len() as u64,
    })
}
