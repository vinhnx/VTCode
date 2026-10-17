//! Image processing utilities
//!
//! Re-exports from `vtcode-commons::image` plus URL-based image fetching.

use anyhow::{Context, Result};
pub use vtcode_commons::image::*;

/// Reads an image from a URL and converts it to base64 format
pub async fn read_image_from_url(url: &str) -> Result<ImageData> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(anyhow::anyhow!("Invalid URL: {url}"));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch image from URL: {url}"))?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!("HTTP error when fetching image: {} (status: {})", url, response.status()));
    }

    let content_length = response.content_length().unwrap_or(0);
    if content_length > 20 * 1024 * 1024 {
        return Err(anyhow::anyhow!("Image from URL too large: {content_length} bytes (max 20MB)"));
    }

    let declared_mime_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(detect_mime_type_from_content_type);

    // A declared but unsupported image type (SVG/BMP/TIFF) must fail here
    // instead of falling through to magic-byte sniffing, which would
    // mislabel SVG markup as PNG and produce a provider 400 downstream.
    if let Some(content_type) = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        && content_type.to_lowercase().starts_with("image/")
        && declared_mime_type
            .as_deref()
            .is_none_or(|mime| !is_supported_image_mime_type(mime))
    {
        return Err(anyhow::anyhow!("Unsupported image content type for LLM vision: {content_type}"));
    }

    let mime_type = declared_mime_type;

    let file_contents = response
        .bytes()
        .await
        .with_context(|| format!("Failed to read response body from: {url}"))?
        .to_vec();

    // Reject markup/error pages that slipped past the header check: SVG and
    // HTML payloads are text, not raster images, and providers refuse them.
    let leading = file_contents.iter().take_while(|byte| byte.is_ascii_whitespace()).count();
    if file_contents.len() > leading && file_contents[leading] == b'<' {
        return Err(anyhow::anyhow!("URL did not return raster image data: {url}"));
    }

    let mime_type = mime_type.unwrap_or_else(|| detect_mime_type_from_data(&file_contents));
    if !is_supported_image_mime_type(&mime_type) {
        return Err(anyhow::anyhow!("Unsupported image content type for LLM vision: {mime_type}"));
    }
    let base64_data = encode_to_base64(&file_contents);

    Ok(ImageData {
        base64_data,
        mime_type,
        file_path: url.to_string(),
        size: file_contents.len() as u64,
    })
}
