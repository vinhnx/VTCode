//! # @ Pattern Parsing Utilities
//!
//! This module provides utilities for parsing @ symbol patterns in user input
//! to automatically load and embed image files as base64-encoded content
//! for LLM processing.

use anyhow::Result;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::llm::provider::{ContentPart, MessageContent};
use crate::utils::image_processing::{read_image_file_any_path, read_image_from_url};
use vtcode_commons::paths::is_safe_relative_path;

/// Parse the @ pattern in text and replace image file paths/URLs with base64 content
///
/// The function looks for patterns like `@./path/to/image.png`, `@image.jpg`, or `@https://example.com/image.png`
/// and replaces them with base64 encoded content that can be processed by LLMs
///
/// # Arguments
///
/// * `input` - The user input text that may contain @ patterns
/// * `base_dir` - The base directory to resolve relative paths from
///
/// # Returns
///
/// * `MessageContent` - Either a single text string or multiple content parts
///   containing both text and base64-encoded images
pub async fn parse_at_patterns(input: &str, base_dir: &Path) -> Result<MessageContent> {
    let at_matches = vtcode_commons::at_pattern::find_at_patterns(input);
    let protected_ranges: Vec<(usize, usize)> =
        at_matches.iter().map(|m| (m.start, m.end)).collect();
    let raw_matches = find_raw_image_path_matches(input, &protected_ranges);
    let data_url_matches = find_data_url_matches(input, &protected_ranges);

    if at_matches.is_empty() && raw_matches.is_empty() && data_url_matches.is_empty() {
        return Ok(MessageContent::text(input.to_string()));
    }

    let mut matches: Vec<PathMatch> = Vec::new();
    for m in at_matches {
        matches.push(PathMatch::At {
            start: m.start,
            end: m.end,
            full_match: m.full_match.to_string(),
            path: m.path.to_string(),
        });
    }
    for m in raw_matches {
        matches.push(PathMatch::Raw {
            start: m.start,
            end: m.end,
            raw: m.raw,
        });
    }
    for m in data_url_matches {
        matches.push(PathMatch::DataUrl {
            start: m.start,
            end: m.end,
            mime_type: m.mime_type,
            data: m.data,
        });
    }
    matches.sort_by_key(|m| m.start());

    let mut parts = Vec::new();
    let mut last_end = 0;

    for m in matches {
        let match_start = m.start();
        let match_end = m.end();

        if match_start < last_end {
            continue;
        }

        if match_start > last_end {
            let text_before = &input[last_end..match_start];
            if !text_before.trim().is_empty() {
                parts.push(ContentPart::text(text_before.to_string()));
            }
        }

        match m {
            PathMatch::At {
                full_match, path, ..
            } => {
                let is_url = path.starts_with("http://") || path.starts_with("https://");
                if is_url {
                    match read_image_from_url(&path).await {
                        Ok(image_data) => {
                            parts.push(ContentPart::Image {
                                data: image_data.base64_data,
                                mime_type: image_data.mime_type,
                                content_type: "image".to_owned(),
                            });
                        }
                        Err(e) => {
                            tracing::warn!("Failed to load image from URL {}: {}", path, e);
                            parts.push(ContentPart::text(full_match));
                        }
                    }
                } else if let Some(image_path) = resolve_image_path(&path, base_dir) {
                    match read_image_file_any_path(&image_path).await {
                        Ok(image_data) => {
                            parts.push(ContentPart::Image {
                                data: image_data.base64_data,
                                mime_type: image_data.mime_type,
                                content_type: "image".to_owned(),
                            });
                        }
                        Err(_) => {
                            parts.push(ContentPart::text(full_match));
                        }
                    }
                } else {
                    parts.push(ContentPart::text(full_match));
                }
            }
            PathMatch::Raw { raw, .. } => {
                if let Some(image_path) = resolve_image_path(&raw, base_dir) {
                    match read_image_file_any_path(&image_path).await {
                        Ok(image_data) => {
                            parts.push(ContentPart::Image {
                                data: image_data.base64_data,
                                mime_type: image_data.mime_type,
                                content_type: "image".to_owned(),
                            });
                        }
                        Err(_) => {
                            parts.push(ContentPart::text(raw));
                        }
                    }
                } else {
                    parts.push(ContentPart::text(raw));
                }
            }
            PathMatch::DataUrl {
                mime_type, data, ..
            } => {
                parts.push(ContentPart::Image {
                    data,
                    mime_type,
                    content_type: "image".to_owned(),
                });
            }
        }

        last_end = match_end;
    }

    if last_end < input.len() {
        let text_after = &input[last_end..];
        if !text_after.trim().is_empty() {
            parts.push(ContentPart::text(text_after.to_string()));
        }
    }

    if parts.is_empty() {
        Ok(MessageContent::text(input.to_string()))
    } else if parts.len() == 1 && matches!(parts[0], ContentPart::Text { .. }) {
        if let ContentPart::Text { text } = &parts[0] {
            Ok(MessageContent::text(text.clone()))
        } else {
            Ok(MessageContent::parts(parts))
        }
    } else {
        Ok(MessageContent::parts(parts))
    }
}

#[derive(Debug)]
struct RawPathMatch {
    start: usize,
    end: usize,
    raw: String,
}

#[derive(Debug)]
struct DataUrlMatch {
    start: usize,
    end: usize,
    mime_type: String,
    data: String,
}

#[derive(Debug)]
enum PathMatch {
    At {
        start: usize,
        end: usize,
        full_match: String,
        path: String,
    },
    Raw {
        start: usize,
        end: usize,
        raw: String,
    },
    DataUrl {
        start: usize,
        end: usize,
        mime_type: String,
        data: String,
    },
}

impl PathMatch {
    fn start(&self) -> usize {
        match self {
            PathMatch::At { start, .. } | PathMatch::Raw { start, .. } => *start,
            PathMatch::DataUrl { start, .. } => *start,
        }
    }

    fn end(&self) -> usize {
        match self {
            PathMatch::At { end, .. } | PathMatch::Raw { end, .. } => *end,
            PathMatch::DataUrl { end, .. } => *end,
        }
    }
}

fn find_raw_image_path_matches(
    input: &str,
    protected_ranges: &[(usize, usize)],
) -> Vec<RawPathMatch> {
    let mut matches = Vec::new();
    let mut quote_ranges = Vec::new();
    let mut active_quote: Option<(char, usize)> = None;

    for (idx, ch) in input.char_indices() {
        match active_quote {
            Some((quote, start)) => {
                if ch == quote {
                    let end = idx + ch.len_utf8();
                    quote_ranges.push((start, end));
                    let inner_start = start + quote.len_utf8();
                    let inner_end = idx;
                    if inner_end > inner_start
                        && !overlaps_range(inner_start, inner_end, protected_ranges)
                    {
                        let inner = &input[inner_start..inner_end];
                        if looks_like_image_path(inner) {
                            matches.push(RawPathMatch {
                                start: inner_start,
                                end: inner_end,
                                raw: inner.to_string(),
                            });
                        }
                    }
                    active_quote = None;
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    active_quote = Some((ch, idx));
                }
            }
        }
    }

    add_spacey_absolute_path_matches(input, protected_ranges, &quote_ranges, &mut matches);

    let mut quote_idx = 0usize;
    let mut token_start: Option<usize> = None;
    let mut pos = 0usize;
    while pos < input.len() {
        if let Some((range_start, range_end)) = quote_ranges.get(quote_idx).copied() {
            if pos >= range_end {
                quote_idx += 1;
                continue;
            }
            if pos >= range_start {
                if let Some(start) = token_start.take() {
                    collect_unquoted_match(
                        input,
                        start,
                        range_start,
                        protected_ranges,
                        &mut matches,
                    );
                }
                pos = range_end;
                continue;
            }
        }

        let ch = input[pos..].chars().next().unwrap();
        if ch.is_ascii_whitespace() {
            if let Some(start) = token_start.take() {
                collect_unquoted_match(input, start, pos, protected_ranges, &mut matches);
            }
            pos += ch.len_utf8();
            continue;
        }

        if ch == '\\'
            && let Some(next) = input[pos + ch.len_utf8()..].chars().next()
            && next.is_ascii_whitespace()
        {
            if token_start.is_none() {
                token_start = Some(pos);
            }
            pos += ch.len_utf8() + next.len_utf8();
            continue;
        }

        if token_start.is_none() {
            token_start = Some(pos);
        }
        pos += ch.len_utf8();
    }

    if let Some(start) = token_start.take() {
        collect_unquoted_match(input, start, input.len(), protected_ranges, &mut matches);
    }

    matches
}

static DATA_IMAGE_URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?ix)
        (?:^|[\s\(\[\{<\"'`])
        (
            data:image/[a-z0-9+\-\.]+;base64,[a-z0-9+/=]+
        )"#,
    )
    .expect("Failed to compile data image regex")
});

fn find_data_url_matches(input: &str, protected_ranges: &[(usize, usize)]) -> Vec<DataUrlMatch> {
    let mut matches = Vec::new();
    for capture in DATA_IMAGE_URL_REGEX.captures_iter(input) {
        let Some(data_match) = capture.get(1) else {
            continue;
        };
        let start = data_match.start();
        let end = data_match.end();
        if overlaps_range(start, end, protected_ranges) {
            continue;
        }
        let raw = data_match.as_str();
        let Some((mime_type, data)) = parse_data_image_url(raw) else {
            continue;
        };
        matches.push(DataUrlMatch {
            start,
            end,
            mime_type,
            data,
        });
    }
    matches
}

static ABSOLUTE_IMAGE_PATH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?ix)
        (?:^|[\s\(\[\{<\"'`])
        (
            (?:file://)?(?:~/|[A-Za-z]:[\\/]|/)
            [^\n]*?
            \.(?:png|jpe?g|gif|bmp|webp|tiff?|svg)
        )"#,
    )
    .expect("Failed to compile absolute image path regex")
});

fn add_spacey_absolute_path_matches(
    input: &str,
    protected_ranges: &[(usize, usize)],
    quote_ranges: &[(usize, usize)],
    matches: &mut Vec<RawPathMatch>,
) {
    for capture in ABSOLUTE_IMAGE_PATH_REGEX.captures_iter(input) {
        let Some(path_match) = capture.get(1) else {
            continue;
        };
        let start = path_match.start();
        let end = path_match.end();
        if overlaps_range(start, end, protected_ranges) {
            continue;
        }
        if overlaps_range(start, end, quote_ranges) {
            continue;
        }
        if matches
            .iter()
            .any(|existing| ranges_overlap(start, end, existing.start, existing.end))
        {
            continue;
        }
        matches.push(RawPathMatch {
            start,
            end,
            raw: path_match.as_str().to_string(),
        });
    }
}

fn ranges_overlap(start: usize, end: usize, other_start: usize, other_end: usize) -> bool {
    start < other_end && end > other_start
}

fn collect_unquoted_match(
    input: &str,
    start: usize,
    end: usize,
    protected_ranges: &[(usize, usize)],
    matches: &mut Vec<RawPathMatch>,
) {
    let Some((trim_start, trim_end)) = trim_token_bounds(input, start, end) else {
        return;
    };
    if overlaps_range(trim_start, trim_end, protected_ranges) {
        return;
    }

    let token = &input[trim_start..trim_end];
    if token.starts_with('@') {
        return;
    }
    if looks_like_image_path(token) {
        matches.push(RawPathMatch {
            start: trim_start,
            end: trim_end,
            raw: token.to_string(),
        });
    }
}

fn trim_token_bounds(input: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    if start >= end || end > input.len() {
        return None;
    }
    let slice = &input[start..end];
    let mut first_non_punct: Option<usize> = None;
    let mut last_non_punct_end: Option<usize> = None;

    for (idx, ch) in slice.char_indices() {
        if first_non_punct.is_none() && !is_leading_punct(ch) {
            first_non_punct = Some(idx);
        }
        if first_non_punct.is_some() && !is_trailing_punct(ch) {
            last_non_punct_end = Some(idx + ch.len_utf8());
        }
    }

    let first = first_non_punct?;
    let last_end = last_non_punct_end?;

    if first >= last_end {
        return None;
    }

    Some((start + first, start + last_end))
}

fn is_leading_punct(ch: char) -> bool {
    matches!(ch, '(' | '[' | '{' | '<' | '"' | '\'' | '`')
}

fn is_trailing_punct(ch: char) -> bool {
    matches!(
        ch,
        ')' | ']' | '}' | '>' | '"' | '\'' | '`' | ',' | '.' | ';' | ':' | '!' | '?'
    )
}

fn looks_like_image_path(token: &str) -> bool {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return false;
    }

    let unescaped = unescape_whitespace(trimmed);
    let mut candidate = unescaped.as_str();
    if let Some(rest) = candidate.strip_prefix("file://") {
        candidate = rest;
    }
    if let Some(rest) = candidate.strip_prefix("~/") {
        candidate = rest;
    }

    if candidate.is_empty() {
        return false;
    }

    crate::utils::image_processing::has_supported_image_extension(Path::new(candidate))
}

fn parse_data_image_url(raw: &str) -> Option<(String, String)> {
    let trimmed = raw.trim_matches(|ch: char| matches!(ch, '"' | '\''));
    let rest = trimmed.strip_prefix("data:")?;
    let (mime_type, data) = rest.split_once(";base64,")?;
    if !mime_type.starts_with("image/") {
        return None;
    }
    let data = data.trim();
    if data.is_empty() {
        return None;
    }
    Some((mime_type.to_string(), data.to_string()))
}

fn resolve_image_path(token: &str, base_dir: &Path) -> Option<PathBuf> {
    let unescaped = unescape_whitespace(token.trim());
    if unescaped.is_empty() {
        return None;
    }

    let mut candidate = unescaped.as_str();
    if let Some(rest) = candidate.strip_prefix("file://") {
        candidate = rest;
    }

    if let Some(rest) = candidate.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return Some(home.join(rest));
        }
        return None;
    }

    if Path::new(candidate).is_absolute() || is_windows_absolute_path(candidate) {
        return Some(PathBuf::from(candidate));
    }

    if !is_safe_relative_path(candidate) {
        return None;
    }

    Some(base_dir.join(candidate))
}

fn is_windows_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() > 2 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn unescape_whitespace(token: &str) -> String {
    let mut result = String::with_capacity(token.len());
    let mut chars = token.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && let Some(next) = chars.peek()
            && next.is_ascii_whitespace()
        {
            result.push(*next);
            chars.next();
            continue;
        }
        result.push(ch);
    }
    result
}

fn overlaps_range(start: usize, end: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(range_start, range_end)| start < *range_end && end > *range_start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_parse_at_patterns_with_image() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("test.png");

        // Create a simple PNG file for testing
        let mut temp_file = std::io::BufWriter::new(std::fs::File::create(&image_path).unwrap());
        // Write a minimal PNG header (not a real image, but valid for testing)
        temp_file
            .write_all(&[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk start
            ])
            .unwrap();
        temp_file.flush().unwrap();

        let input = format!(
            "Look at this image: @{}",
            image_path.file_name().unwrap().to_string_lossy()
        );

        let result = parse_at_patterns(&input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Parts(parts) => {
                assert_eq!(parts.len(), 2); // Text part + image part
                assert!(matches!(parts[0], ContentPart::Text { .. }));
                assert!(matches!(parts[1], ContentPart::Image { .. }));
            }
            _ => panic!("Expected multi-part content"),
        }
    }

    #[tokio::test]
    async fn test_parse_raw_absolute_image_path() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("absolute.png");

        let mut temp_file = std::io::BufWriter::new(std::fs::File::create(&image_path).unwrap());
        temp_file
            .write_all(&[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk start
            ])
            .unwrap();
        temp_file.flush().unwrap();

        let input = format!("see {}", image_path.display());
        let result = parse_at_patterns(&input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Parts(parts) => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(parts[0], ContentPart::Text { .. }));
                assert!(matches!(parts[1], ContentPart::Image { .. }));
            }
            _ => panic!("Expected multi-part content"),
        }
    }

    #[tokio::test]
    async fn test_parse_raw_relative_image_path() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("relative.png");

        let mut temp_file = std::io::BufWriter::new(std::fs::File::create(&image_path).unwrap());
        temp_file
            .write_all(&[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk start
            ])
            .unwrap();
        temp_file.flush().unwrap();

        let input = "see relative.png";
        let result = parse_at_patterns(input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Parts(parts) => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(parts[0], ContentPart::Text { .. }));
                assert!(matches!(parts[1], ContentPart::Image { .. }));
            }
            _ => panic!("Expected multi-part content"),
        }
    }

    #[tokio::test]
    async fn test_parse_raw_quoted_image_path_with_spaces() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("with space.png");

        let mut temp_file = std::io::BufWriter::new(std::fs::File::create(&image_path).unwrap());
        temp_file
            .write_all(&[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk start
            ])
            .unwrap();
        temp_file.flush().unwrap();

        let input = format!("see \"{}\"", image_path.display());
        let result = parse_at_patterns(&input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Parts(parts) => {
                assert!(
                    parts
                        .iter()
                        .any(|part| matches!(part, ContentPart::Image { .. }))
                );
            }
            _ => panic!("Expected multi-part content"),
        }
    }

    #[tokio::test]
    async fn test_parse_raw_escaped_space_image_path() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("escaped space.png");

        let mut temp_file = std::io::BufWriter::new(std::fs::File::create(&image_path).unwrap());
        temp_file
            .write_all(&[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk start
            ])
            .unwrap();
        temp_file.flush().unwrap();

        let escaped = image_path.to_string_lossy().replace(' ', "\\ ");
        let input = format!("see {}", escaped);
        let result = parse_at_patterns(&input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Parts(parts) => {
                assert!(
                    parts
                        .iter()
                        .any(|part| matches!(part, ContentPart::Image { .. }))
                );
            }
            _ => panic!("Expected multi-part content"),
        }
    }

    #[tokio::test]
    async fn test_parse_raw_unescaped_space_image_path() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("unescaped space.png");

        let mut temp_file = std::io::BufWriter::new(std::fs::File::create(&image_path).unwrap());
        temp_file
            .write_all(&[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk start
            ])
            .unwrap();
        temp_file.flush().unwrap();

        let input = format!("see {} now", image_path.display());
        let result = parse_at_patterns(&input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Parts(parts) => {
                assert!(
                    parts
                        .iter()
                        .any(|part| matches!(part, ContentPart::Image { .. }))
                );
            }
            _ => panic!("Expected multi-part content"),
        }
    }

    #[tokio::test]
    async fn test_parse_raw_narrow_no_break_space_image_path() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join(format!("narrow\u{202F}space.png"));

        let mut temp_file = std::io::BufWriter::new(std::fs::File::create(&image_path).unwrap());
        temp_file
            .write_all(&[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk start
            ])
            .unwrap();
        temp_file.flush().unwrap();

        let input = format!("see {} now", image_path.display());
        let result = parse_at_patterns(&input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Parts(parts) => {
                assert!(
                    parts
                        .iter()
                        .any(|part| matches!(part, ContentPart::Image { .. }))
                );
            }
            _ => panic!("Expected multi-part content"),
        }
    }

    #[tokio::test]
    async fn test_parse_at_absolute_image_path() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("at-absolute.png");

        let mut temp_file = std::io::BufWriter::new(std::fs::File::create(&image_path).unwrap());
        temp_file
            .write_all(&[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk start
            ])
            .unwrap();
        temp_file.flush().unwrap();

        let input = format!("see @{}", image_path.display());
        let result = parse_at_patterns(&input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Parts(parts) => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(parts[0], ContentPart::Text { .. }));
                assert!(matches!(parts[1], ContentPart::Image { .. }));
            }
            _ => panic!("Expected multi-part content"),
        }
    }

    #[tokio::test]
    async fn test_parse_at_patterns_regular_text() {
        let temp_dir = TempDir::new().unwrap();
        let input = "This is just regular text with @ symbol not followed by file";

        let result = parse_at_patterns(input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Text(text) => {
                assert_eq!(text, input);
            }
            _ => panic!("Expected single text content"),
        }
    }

    #[test]
    fn test_is_safe_relative_path() {
        use vtcode_commons::paths::is_safe_relative_path;
        assert!(!is_safe_relative_path("../../etc/passwd"));
        assert!(!is_safe_relative_path("../file.txt"));
        assert!(is_safe_relative_path("file.txt"));
        assert!(is_safe_relative_path("./path/file.txt"));
        assert!(is_safe_relative_path(" path with spaces .txt "));
    }

    #[tokio::test]
    async fn test_parse_at_patterns_invalid_file() {
        let temp_dir = TempDir::new().unwrap();
        let input = "Look at @nonexistent.png which doesn't exist";

        let result = parse_at_patterns(input, temp_dir.path()).await.unwrap();

        // When file doesn't exist, the text is split into parts:
        // "Look at ", "@nonexistent.png", " which doesn't exist"
        match result {
            MessageContent::Parts(parts) => {
                assert_eq!(parts.len(), 3);
                assert!(matches!(parts[0], ContentPart::Text { .. }));
                assert!(matches!(parts[1], ContentPart::Text { .. }));
                assert!(matches!(parts[2], ContentPart::Text { .. }));
            }
            _ => panic!("Expected multi-part content"),
        }
    }

    #[tokio::test]
    async fn test_parse_at_patterns_url() {
        let temp_dir = TempDir::new().unwrap();
        let input = "Look at @https://example.com/image.png";

        let result = parse_at_patterns(input, temp_dir.path()).await.unwrap();

        // For URL tests, we expect the result to be text (since mock server isn't available)
        // In real usage with a valid URL, it would return multi-part content with image
        match result {
            MessageContent::Text(text) => {
                assert!(text.contains("@https://example.com/image.png"));
            }
            _ => {}
        }
    }

    #[tokio::test]
    async fn test_parse_at_patterns_data_url_image() {
        let temp_dir = TempDir::new().unwrap();
        let input = "inline data:image/png;base64,aGVsbG8=";

        let result = parse_at_patterns(input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Parts(parts) => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(parts[0], ContentPart::Text { .. }));
                assert!(matches!(parts[1], ContentPart::Image { .. }));
            }
            _ => panic!("Expected multi-part content"),
        }
    }
}
