//! File input helpers for provider-specific inline file attachments.

use anyhow::{Context, Result};
use base64::Engine as _;
use std::path::Path;

pub const MAX_INPUT_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// File data prepared for inline model input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInputData {
    pub base64_data: String,
    pub filename: String,
    pub file_path: String,
    pub size: u64,
}

/// Read a validated local file path for inline model input.
///
/// Callers must validate path scope and user intent before using this helper.
pub async fn read_input_file_any_path<P: AsRef<Path>>(file_path: P) -> Result<FileInputData> {
    let path = file_path.as_ref();
    let metadata = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("Failed to stat input file: {}", path.display()))?;

    if !metadata.is_file() {
        return Err(anyhow::anyhow!(
            "Input path is not a file: {}",
            path.display()
        ));
    }

    if metadata.len() > MAX_INPUT_FILE_BYTES {
        return Err(anyhow::anyhow!(
            "Input file too large: {} bytes (max {} bytes)",
            metadata.len(),
            MAX_INPUT_FILE_BYTES
        ));
    }

    let file_contents = tokio::fs::read(path)
        .await
        .with_context(|| format!("Failed to read input file: {}", path.display()))?;

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.display().to_string());

    Ok(FileInputData {
        base64_data: base64::engine::general_purpose::STANDARD.encode(&file_contents),
        filename,
        file_path: path.display().to_string(),
        size: file_contents.len() as u64,
    })
}

pub fn decoded_base64_size(file_data: &str) -> Result<u64> {
    let payload = inline_base64_payload(file_data);
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .context("Invalid base64 file_data payload")?;
    Ok(decoded.len() as u64)
}

fn inline_base64_payload(file_data: &str) -> &str {
    let trimmed = file_data.trim();
    if let Some((prefix, payload)) = trimmed.split_once(',')
        && prefix.contains(";base64")
    {
        payload.trim()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_INPUT_FILE_BYTES, decoded_base64_size};

    #[test]
    fn decoded_base64_size_supports_raw_base64() {
        assert_eq!(decoded_base64_size("aGVsbG8=").unwrap(), 5);
    }

    #[test]
    fn decoded_base64_size_supports_data_url_prefix() {
        assert_eq!(
            decoded_base64_size("data:application/pdf;base64,aGVsbG8=").unwrap(),
            5
        );
    }

    #[test]
    fn max_input_file_bytes_matches_openai_limit() {
        assert_eq!(MAX_INPUT_FILE_BYTES, 50 * 1024 * 1024);
    }
}
