use std::io::ErrorKind;
use std::path::Path;

use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};

use super::super::matcher::{PatchContextMatcher, redacted_context_matches, seek_segment};
use super::super::semantic::{resolve_semantic_match, semantic_anchor_terms};
use super::io::AtomicWriter;
use super::{PatchChunk, PatchError};

use vtcode_commons::preview::condense_error_echo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LineEnding {
    LF,
    Crlf,
}

impl LineEnding {
    pub fn as_str(&self) -> &'static str {
        match self {
            LineEnding::LF => "\n",
            LineEnding::Crlf => "\r\n",
        }
    }
}

pub(super) async fn load_file_lines(path: &Path) -> Result<(Vec<String>, bool, LineEnding), PatchError> {
    let file = fs::File::open(path).await.map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            PatchError::MissingFile { path: path.display().to_string() }
        } else {
            PatchError::Io {
                action: "read",
                path: path.to_path_buf(),
                source: err,
            }
        }
    })?;
    let mut reader = BufReader::new(file);
    let mut lines = Vec::new();
    let mut had_trailing_newline = false;
    let mut line_ending = LineEnding::LF;
    let mut detected_ending = false;

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await.map_err(|err| PatchError::Io {
            action: "read",
            path: path.to_path_buf(),
            source: err,
        })?;

        if bytes_read == 0 {
            break;
        }

        if line.ends_with('\n') {
            had_trailing_newline = true;
            if !detected_ending {
                if line.ends_with("\r\n") {
                    line_ending = LineEnding::Crlf;
                }
                detected_ending = true;
            }
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

    Ok((lines, had_trailing_newline, line_ending))
}

fn lines_from_content(content: &str) -> (Vec<String>, bool, LineEnding) {
    let had_trailing_newline = content.ends_with('\n');
    let line_ending = if content.contains("\r\n") {
        LineEnding::Crlf
    } else {
        LineEnding::LF
    };
    let lines = content.lines().map(ToOwned::to_owned).collect();
    (lines, had_trailing_newline, line_ending)
}

pub(super) async fn compute_replacements(
    source_path: &Path,
    original_lines: &[String],
    chunks: &[PatchChunk],
    path: &str,
) -> Result<Vec<(usize, usize, Vec<String>)>, PatchError> {
    Ok(compute_replacements_with_inspection(source_path, original_lines, chunks, path)
        .await?
        .replacements)
}

struct ReplacementPlan {
    replacements: Vec<(usize, usize, Vec<String>)>,
    required_ranges: Vec<std::ops::Range<usize>>,
}

async fn compute_replacements_with_inspection(
    source_path: &Path,
    original_lines: &[String],
    chunks: &[PatchChunk],
    path: &str,
) -> Result<ReplacementPlan, PatchError> {
    let mut inspected = Vec::new();
    let matcher = PatchContextMatcher::new(original_lines);
    let mut replacements = Vec::new();
    let mut line_index = 0usize;

    for chunk in chunks {
        let mut context_found = true;
        let mut redacted_anchor = false;
        if let Some(hint_line) = chunk.parse_line_number() {
            line_index = hint_line.saturating_sub(1);
        } else if let Some(context) = chunk.change_context() {
            let search_pattern = vec![context.to_string()];
            if let Some(idx) = matcher.seek(&search_pattern, line_index, false) {
                inspected.push(idx..idx + 1);
                line_index = idx + 1;
            } else {
                context_found = false;
                redacted_anchor = redacted_context_matches(original_lines, &search_pattern, line_index, false);
            }
        }

        let (mut old_segment, mut new_segment) = chunk.to_segments();

        if !chunk.has_old_lines() {
            if redacted_anchor {
                return Err(PatchError::RedactedContext { path: path.to_string() });
            }
            if !context_found && let Some(context) = chunk.change_context() {
                return Err(PatchError::ContextNotFound {
                    path: path.to_string(),
                    context: condense_error_echo(context),
                });
            }
            let insertion_idx = if chunk.change_context().is_some() {
                line_index.min(original_lines.len())
            } else {
                original_lines.len()
            };

            inspected.push(insertion_idx.saturating_sub(1)..(insertion_idx + 1).min(original_lines.len()));
            line_index = insertion_idx.saturating_add(new_segment.len());
            replacements.push((insertion_idx, 0, new_segment));
            continue;
        }

        let mut found = if context_found {
            seek_segment(&matcher, &mut old_segment, &mut new_segment, line_index, chunk.is_end_of_file())
        } else {
            None
        };

        let mut redacted_match = redacted_anchor
            || (found.is_none()
                && context_found
                && redacted_context_matches(original_lines, &old_segment, line_index, chunk.is_end_of_file()));

        // The semantic path is a fallback: it runs only when the literal
        // context search failed and the anchor names a symbol. Computing the
        // anchor terms compiles the language's declaration patterns, so it is
        // left to the path that needs it.
        if found.is_none()
            && let Some(anchor_terms) = chunk
                .change_context()
                .map(|context| semantic_anchor_terms(source_path, context))
                .filter(|terms| !terms.is_empty())
        {
            match resolve_semantic_match(
                source_path,
                path,
                original_lines,
                chunk,
                &anchor_terms,
                old_segment.clone(),
                new_segment.clone(),
            ) {
                Ok(semantic) => {
                    found = Some(semantic.start_idx);
                    old_segment = semantic.old_segment;
                    new_segment = semantic.new_segment;
                }
                Err(PatchError::SemanticResolutionFailed { reason, .. }) if reason.contains("unsupported language") => {
                    // The language has no declaration support (e.g. markdown,
                    // yaml). Fall back to exact text search from the beginning
                    // of the file.
                    found = seek_segment(&matcher, &mut old_segment, &mut new_segment, 0, chunk.is_end_of_file());
                    redacted_match |= found.is_none()
                        && redacted_context_matches(original_lines, &old_segment, 0, chunk.is_end_of_file());
                }
                Err(PatchError::SemanticResolutionFailed { .. }) if redacted_match => {
                    return Err(PatchError::RedactedContext { path: path.to_string() });
                }
                Err(err) => return Err(err),
            }
        }

        if let Some(start_idx) = found {
            inspected.push(start_idx..start_idx + old_segment.len());
            if !context_found {
                inspected.push(0..original_lines.len());
            }
            line_index = start_idx + old_segment.len();
            replacements.push((start_idx, old_segment.len(), new_segment));
        } else {
            if redacted_match {
                return Err(PatchError::RedactedContext { path: path.to_string() });
            }
            if !context_found && let Some(context) = chunk.change_context() {
                return Err(PatchError::ContextNotFound {
                    path: path.to_string(),
                    context: condense_error_echo(context),
                });
            }
            let snippet = if old_segment.is_empty() {
                "<empty>".to_string()
            } else {
                condense_error_echo(&old_segment.join("\n"))
            };
            return Err(PatchError::SegmentNotFound { path: path.to_string(), snippet });
        }
    }

    replacements.sort_by_key(|(idx, _, _)| *idx);
    // Line-number hints can move the search backward, so two chunks may match
    // overlapping ranges. Rendering those would silently double-apply; fail
    // loud instead. Zero-length insertions at the same point are unambiguous
    // and stay allowed.
    let mut covered_end = 0usize;
    for (start_idx, old_len, _) in &replacements {
        if *start_idx < covered_end {
            return Err(PatchError::InvalidOperation {
                path: path.to_string(),
                reason: "patch chunks match overlapping file ranges; disambiguate the context lines".to_string(),
            });
        }
        covered_end = start_idx.saturating_add(*old_len);
    }
    Ok(ReplacementPlan { replacements, required_ranges: inspected })
}

pub(super) async fn render_patched_text_from_content(
    source_path: &Path,
    content: &str,
    chunks: &[PatchChunk],
    path: &str,
) -> Result<String, PatchError> {
    let (original_lines, had_trailing_newline, line_ending) = lines_from_content(content);
    let replacements = compute_replacements(source_path, &original_lines, chunks, path).await?;
    let ensure_trailing_newline = had_trailing_newline || chunks.iter().any(|chunk| chunk.is_end_of_file());
    Ok(render_patched_content(original_lines, replacements, ensure_trailing_newline, line_ending))
}

/// The ordered line slices of the patched document, borrowed from the
/// immutable original and the replacement segments.
///
/// This is the single edit-application pass: one walk over the collected
/// replacements builds the result without copying line text. Both the
/// in-memory renderer and the streaming writer consume it, so there is exactly
/// one definition of how edits are applied — mirroring the `SyntaxEditor`
/// approach of collecting edits and building a fresh document from them.
fn patched_line_segments<'a>(
    original_lines: &'a [String],
    replacements: &'a [(usize, usize, Vec<String>)],
) -> Vec<&'a [String]> {
    // # Requires
    // - `replacements` is sorted by start index and non-overlapping (as
    //   produced by `compute_replacements_with_inspection`).
    // # Ensures
    // - The returned slices, concatenated, are the patched document's lines.
    // - Total (terminating): one pass over `replacements`.
    let mut segments = Vec::with_capacity(replacements.len().saturating_mul(2).saturating_add(1));
    let mut cursor = 0usize;
    for (start_idx, old_len, new_segment) in replacements {
        let start = *start_idx;
        debug_assert!(
            cursor <= start && start <= original_lines.len(),
            "replacement plan must be ordered and in bounds"
        );
        if cursor < start {
            segments.push(&original_lines[cursor..start]);
        }
        segments.push(new_segment.as_slice());
        cursor = start.saturating_add(*old_len);
    }
    if cursor < original_lines.len() {
        segments.push(&original_lines[cursor..]);
    }
    segments
}

/// One trailing-newline verdict shared by both application paths.
fn needs_trailing_ending(segments: &[&[String]], ensure_trailing_newline: bool) -> bool {
    if !ensure_trailing_newline {
        return false;
    }
    // A trailing empty line already contributed the separator that ends the
    // document. For zero lines the empty document gets the ending, matching
    // the previous `last_line_was_empty = false` behaviour.
    segments
        .iter()
        .rev()
        .find_map(|segment| segment.last())
        .is_none_or(|line| !line.is_empty())
}

fn render_patched_content(
    original_lines: Vec<String>,
    replacements: Vec<(usize, usize, Vec<String>)>,
    ensure_trailing_newline: bool,
    line_ending: LineEnding,
) -> String {
    let segments = patched_line_segments(&original_lines, &replacements);
    let ending = line_ending.as_str();
    let estimated_capacity = original_lines
        .iter()
        .map(|line| line.len())
        .sum::<usize>()
        .saturating_add(original_lines.len().saturating_sub(1).saturating_mul(ending.len()))
        .saturating_add(ending.len());
    let mut rendered = String::with_capacity(estimated_capacity);
    let mut first = true;
    for line in segments.iter().flat_map(|segment| segment.iter()) {
        if !first {
            rendered.push_str(ending);
        }
        rendered.push_str(line);
        first = false;
    }
    if needs_trailing_ending(&segments, ensure_trailing_newline) {
        rendered.push_str(ending);
    }
    rendered
}

pub(super) async fn write_patched_content(
    writer: &mut AtomicWriter,
    original_lines: Vec<String>,
    replacements: Vec<(usize, usize, Vec<String>)>,
    ensure_trailing_newline: bool,
    line_ending: LineEnding,
) -> Result<(), PatchError> {
    let segments = patched_line_segments(&original_lines, &replacements);
    let ending = line_ending.as_str();
    let mut first = true;
    for line in segments.iter().flat_map(|segment| segment.iter()) {
        if !first {
            writer.write_all(ending.as_bytes()).await?;
        }
        writer.write_all(line.as_bytes()).await?;
        first = false;
    }
    if needs_trailing_ending(&segments, ensure_trailing_newline) {
        writer.write_all(ending.as_bytes()).await?;
    }
    Ok(())
}

pub(crate) async fn inspection_replacements(
    source_path: &Path,
    content: &str,
    chunks: &[PatchChunk],
    path: &str,
) -> Result<crate::tools::source_inspection::mutations::PatchInspection, PatchError> {
    let (lines, _, _) = lines_from_content(content);
    let plan = compute_replacements_with_inspection(source_path, &lines, chunks, path).await?;
    Ok(crate::tools::source_inspection::mutations::PatchInspection {
        replacements: plan
            .replacements
            .into_iter()
            .map(|(start, removed, added)| crate::tools::source_inspection::mutations::InspectedReplacement {
                start,
                removed,
                added: added.len(),
            })
            .collect(),
        required_ranges: plan.required_ranges,
    })
}

#[cfg(test)]
mod tests {
    use super::super::io::AtomicWriter;
    use super::{
        LineEnding, needs_trailing_ending, patched_line_segments, render_patched_content, write_patched_content,
    };

    #[test]
    fn patched_segments_apply_edits_in_file_order() {
        let original: Vec<String> = ["top", "remove", "keep", "bottom"].iter().map(|s| (*s).to_string()).collect();
        let replacements = vec![
            (1usize, 1usize, vec!["replacement".to_string()]),
            (2usize, 0usize, vec!["first-insert".to_string()]),
            (2usize, 0usize, vec!["second-insert".to_string()]),
            (3usize, 1usize, Vec::new()),
        ];

        let segments = patched_line_segments(&original, &replacements);
        let lines: Vec<&str> = segments.iter().flat_map(|segment| segment.iter()).map(String::as_str).collect();

        assert_eq!(lines, ["top", "replacement", "first-insert", "second-insert", "keep"]);
    }

    #[test]
    fn trailing_ending_ignores_empty_final_line_and_empty_document() {
        let one = vec!["a".to_string()];
        let with_empty_tail = vec!["a".to_string(), String::new()];
        let empty: Vec<String> = Vec::new();
        let segments: Vec<&[String]> = vec![one.as_slice(), with_empty_tail.as_slice(), empty.as_slice()];
        let verdicts: Vec<bool> = segments
            .iter()
            .map(|segment| needs_trailing_ending(&[*segment], true))
            .collect();

        assert_eq!(verdicts, [true, false, true]);
        assert!(!needs_trailing_ending(&[one.as_slice()], false));
    }

    #[tokio::test]
    async fn streaming_writer_matches_in_memory_renderer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let original: Vec<String> = ["alpha", "stale", "omega"].iter().map(|s| (*s).to_string()).collect();
        let replacements = vec![(1usize, 1usize, vec!["fresh".to_string()])];
        let cases = [
            (LineEnding::LF, true),
            (LineEnding::LF, false),
            (LineEnding::Crlf, true),
        ];

        for (line_ending, ensure_trailing_newline) in cases {
            let rendered =
                render_patched_content(original.clone(), replacements.clone(), ensure_trailing_newline, line_ending);

            let path = dir.path().join("patched.txt");
            let mut writer = AtomicWriter::create(&path, None).await.expect("create writer");
            write_patched_content(
                &mut writer,
                original.clone(),
                replacements.clone(),
                ensure_trailing_newline,
                line_ending,
            )
            .await
            .expect("write patched content");
            writer.commit().await.expect("commit");
            let streamed = tokio::fs::read_to_string(&path).await.expect("read patched file");

            assert_eq!(streamed, rendered, "renderer and writer must apply edits identically");
        }
    }
}
