use std::path::Path;

use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};

use super::super::matcher::PatchContextMatcher;
use super::io::AtomicWriter;
use super::{PatchChunk, PatchError};

pub(super) async fn load_file_lines(path: &Path) -> Result<(Vec<String>, bool), PatchError> {
    let file = fs::File::open(path).await.map_err(|err| PatchError::Io {
        action: "read",
        path: path.to_path_buf(),
        source: err,
    })?;
    let mut reader = BufReader::new(file);
    let mut lines = Vec::new();
    let mut had_trailing_newline = false;

    loop {
        let mut line = String::new();
        let bytes_read = reader
            .read_line(&mut line)
            .await
            .map_err(|err| PatchError::Io {
                action: "read",
                path: path.to_path_buf(),
                source: err,
            })?;

        if bytes_read == 0 {
            break;
        }

        if line.ends_with('\n') {
            had_trailing_newline = true;
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        } else {
            had_trailing_newline = false;
        }

        if line.ends_with('\r') {
            line.pop();
        }

        lines.push(line);
    }

    Ok((lines, had_trailing_newline))
}

pub(super) fn compute_replacements(
    original_lines: &[String],
    chunks: &[PatchChunk],
    path: &str,
) -> Result<Vec<(usize, usize, Vec<String>)>, PatchError> {
    let matcher = PatchContextMatcher::new(original_lines);
    let mut replacements = Vec::new();
    let mut line_index = 0usize;

    for chunk in chunks {
        if let Some(context) = chunk.change_context() {
            let search_pattern = vec![context.to_string()];
            if let Some(idx) = matcher.seek(&search_pattern, line_index, false) {
                line_index = idx + 1;
            } else {
                return Err(PatchError::ContextNotFound {
                    path: path.to_string(),
                    context: context.to_string(),
                });
            }
        }

        let (mut old_segment, mut new_segment) = chunk.to_segments();

        if !chunk.has_old_lines() {
            let insertion_idx = if chunk.change_context().is_some() {
                line_index.min(original_lines.len())
            } else {
                original_lines.len()
            };

            line_index = insertion_idx.saturating_add(new_segment.len());
            replacements.push((insertion_idx, 0, new_segment));
            continue;
        }

        let mut found = matcher.seek(&old_segment, line_index, chunk.is_end_of_file());

        if found.is_none() && old_segment.last().is_some_and(|line| line.is_empty()) {
            old_segment.pop();
            if new_segment.last().is_some_and(|line| line.is_empty()) {
                new_segment.pop();
            }
            found = matcher.seek(&old_segment, line_index, chunk.is_end_of_file());
        }

        if let Some(start_idx) = found {
            line_index = start_idx + old_segment.len();
            replacements.push((start_idx, old_segment.len(), new_segment));
        } else {
            let snippet = if old_segment.is_empty() {
                "<empty>".to_string()
            } else {
                old_segment.join("\n")
            };
            return Err(PatchError::SegmentNotFound {
                path: path.to_string(),
                snippet,
            });
        }
    }

    replacements.sort_by_key(|(idx, _, _)| *idx);
    Ok(replacements)
}

pub(super) async fn write_patched_content(
    writer: &mut AtomicWriter,
    original_lines: Vec<String>,
    replacements: Vec<(usize, usize, Vec<String>)>,
    ensure_trailing_newline: bool,
) -> Result<(), PatchError> {
    let new_lines = apply_replacements(original_lines, &replacements);
    let mut content = new_lines.join("\n");
    if ensure_trailing_newline && !content.ends_with('\n') {
        content.push('\n');
    }
    writer.write_all(content.as_bytes()).await
}

fn apply_replacements(
    mut lines: Vec<String>,
    replacements: &[(usize, usize, Vec<String>)],
) -> Vec<String> {
    for (start_idx, old_len, new_segment) in replacements.iter().rev() {
        let start = *start_idx;
        let len = *old_len;

        for _ in 0..len {
            if start < lines.len() {
                lines.remove(start);
            }
        }

        for (offset, value) in new_segment.iter().enumerate() {
            lines.insert(start + offset, value.clone());
        }
    }

    lines
}
