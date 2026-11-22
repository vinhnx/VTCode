use std::io::ErrorKind;
use std::path::Path;

use tokio::fs;

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

/// Load a file through the same splitter as the dry-run/rendering paths.
///
/// `render_updated_content` (conflict previews, approval diffs and the source
/// ledger) works from a `&str`, while `UpdateOperation` used to open the file
/// with a separate line reader. The two splitters disagreed on bare `\r` and
/// on mixed line endings, so a preview could show different bytes from the
/// ones actually written. Reading the file and delegating to
/// `lines_from_content` gives every path one authoritative definition of
/// "lines, trailing newline, line ending".
pub(super) async fn load_file_lines(path: &Path) -> Result<(Vec<String>, bool, LineEnding), PatchError> {
    let content = fs::read_to_string(path).await.map_err(|err| {
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
    Ok(lines_from_content(&content))
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
) -> Result<ReplacementPlan, PatchError> {
    compute_replacements_with_inspection(source_path, original_lines, chunks, path).await
}

/// A validated edit script: replacements are sorted by start index,
/// non-overlapping, and within the bounds of the document they were computed
/// against. `build` establishes those invariants once, so every renderer can
/// apply the script without re-checking them — the same constructor-owned
/// invariant discipline `SyntaxEditor` uses for its edits.
#[derive(Debug)]
pub(super) struct ReplacementPlan {
    replacements: Vec<(usize, usize, Vec<String>)>,
    required_ranges: Vec<std::ops::Range<usize>>,
}

impl ReplacementPlan {
    fn build(
        mut replacements: Vec<(usize, usize, Vec<String>)>,
        required_ranges: Vec<std::ops::Range<usize>>,
        path: &str,
        original_line_count: usize,
    ) -> Result<Self, PatchError> {
        replacements.sort_by_key(|(start, _, _)| *start);
        let mut covered_end = 0usize;
        for (start, removed, _) in &replacements {
            if *start > original_line_count || start.saturating_add(*removed) > original_line_count {
                return Err(PatchError::InvalidOperation {
                    path: path.to_string(),
                    reason: "patch replacement targets lines outside the file".to_string(),
                });
            }
            // Line-number hints can move the search backward, so two chunks
            // may match overlapping ranges. Rendering those would silently
            // double-apply; fail loud. Zero-length insertions at the same
            // point are unambiguous and stay allowed.
            if *start < covered_end {
                return Err(PatchError::OverlappingChunks { path: path.to_string() });
            }
            covered_end = start.saturating_add(*removed);
        }
        Ok(Self { replacements, required_ranges })
    }
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
                &matcher,
                &old_segment,
                &new_segment,
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

    // Sort, validate bounds and reject overlaps exactly once, at construction.
    // Both renderers consume the resulting `ReplacementPlan` and rely on its
    // invariants rather than repeating this sweep.
    ReplacementPlan::build(replacements, inspected, path, original_lines.len())
}

pub(super) async fn render_patched_text_from_content(
    source_path: &Path,
    content: &str,
    chunks: &[PatchChunk],
    path: &str,
) -> Result<String, PatchError> {
    let (original_lines, had_trailing_newline, line_ending) = lines_from_content(content);
    let plan = compute_replacements(source_path, &original_lines, chunks, path).await?;
    let ensure_trailing_newline = had_trailing_newline || chunks.iter().any(|chunk| chunk.is_end_of_file());
    Ok(render_patched_content(original_lines, &plan, ensure_trailing_newline, line_ending))
}

/// The ordered line slices of the patched document, borrowed from the
/// immutable original and the replacement segments.
///
/// This is the single edit-application pass: one walk over the collected
/// replacements builds the result without copying line text. Both the
/// in-memory renderer and the streaming writer consume it, so there is exactly
/// one definition of how edits are applied — mirroring the `SyntaxEditor`
/// approach of collecting edits and building a fresh document from them.
fn patched_line_segments<'a>(original_lines: &'a [String], plan: &'a ReplacementPlan) -> Vec<&'a [String]> {
    // # Ensures
    // - The returned slices, concatenated, are the patched document's lines.
    // - Total (terminating): one pass over the plan's replacements.
    // The plan's sorted/non-overlapping/in-bounds invariants are established by
    // `ReplacementPlan::build`, so consumers never need to re-assert them.
    let replacements = &plan.replacements;
    let mut segments = Vec::with_capacity(replacements.len().saturating_mul(2).saturating_add(1));
    let mut cursor = 0usize;
    for (start_idx, old_len, new_segment) in replacements {
        let start = *start_idx;
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

fn render_patched_content(
    original_lines: Vec<String>,
    plan: &ReplacementPlan,
    ensure_trailing_newline: bool,
    line_ending: LineEnding,
) -> String {
    let segments = patched_line_segments(&original_lines, plan);
    let ending = line_ending.as_str();
    let estimated_capacity = original_lines
        .iter()
        .map(|line| line.len())
        .sum::<usize>()
        .saturating_add(original_lines.len().saturating_sub(1).saturating_mul(ending.len()))
        .saturating_add(
            plan.replacements
                .iter()
                .flat_map(|(_, _, added)| added.iter().map(|line| line.len()))
                .sum::<usize>(),
        )
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
    // `ensure_trailing_newline` is a property of the document, not of its last
    // line. Lines never contain the ending, so appending it unconditionally is
    // the exact inverse of the split in `lines_from_content`/`load_file_lines`:
    // an empty last line is a real line that needs its own terminator.
    if ensure_trailing_newline {
        rendered.push_str(ending);
    }
    rendered
}

pub(super) async fn write_patched_content(
    writer: &mut AtomicWriter,
    original_lines: Vec<String>,
    plan: &ReplacementPlan,
    ensure_trailing_newline: bool,
    line_ending: LineEnding,
) -> Result<(), PatchError> {
    let segments = patched_line_segments(&original_lines, plan);
    let ending = line_ending.as_str();
    let mut first = true;
    for line in segments.iter().flat_map(|segment| segment.iter()) {
        if !first {
            writer.write_all(ending.as_bytes()).await?;
        }
        writer.write_all(line.as_bytes()).await?;
        first = false;
    }
    if ensure_trailing_newline {
        writer.write_all(ending.as_bytes()).await?;
    }
    Ok(())
}

/// Derive the source-inspection ledger entries and the rendered `after`
/// content from a single matched edit script.
///
/// Previously `prepare_patch` matched every hunk once to build the inspection
/// and then again to render the preview — two identical full-file matcher
/// passes per update. Both views now come from the same plan, so the ledger
/// and the preview cannot disagree and the match runs once.
pub(crate) async fn inspect_and_render_patch_update(
    source_path: &Path,
    content: &str,
    chunks: &[PatchChunk],
    path: &str,
) -> Result<(crate::tools::source_inspection::mutations::PatchInspection, String), PatchError> {
    let (original_lines, had_trailing_newline, line_ending) = lines_from_content(content);
    let plan = compute_replacements_with_inspection(source_path, &original_lines, chunks, path).await?;
    let inspection = crate::tools::source_inspection::mutations::PatchInspection {
        replacements: plan
            .replacements
            .iter()
            .map(|(start, removed, added)| crate::tools::source_inspection::mutations::InspectedReplacement {
                start: *start,
                removed: *removed,
                added: added.len(),
            })
            .collect(),
        required_ranges: plan.required_ranges.clone(),
    };
    let ensure_trailing_newline = had_trailing_newline || chunks.iter().any(|chunk| chunk.is_end_of_file());
    let rendered = render_patched_content(original_lines, &plan, ensure_trailing_newline, line_ending);
    Ok((inspection, rendered))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::tools::editing::patch::{Patch, PatchOperation};

    use super::super::io::AtomicWriter;
    use super::{
        LineEnding, PatchError, ReplacementPlan, inspect_and_render_patch_update, patched_line_segments,
        render_patched_content, write_patched_content,
    };

    fn plan(original_len: usize, replacements: Vec<(usize, usize, Vec<String>)>) -> ReplacementPlan {
        ReplacementPlan::build(replacements, Vec::new(), "/fixture", original_len).expect("valid plan")
    }

    #[tokio::test]
    async fn inspection_and_render_derive_from_one_plan() {
        let patch = Patch::parse("*** Begin Patch\n*** Update File: f.txt\n@@\n-stale\n+fresh\n*** End Patch\n")
            .expect("patch parses");
        let chunks = match &patch.operations()[0] {
            PatchOperation::UpdateFile { chunks, .. } => chunks.clone(),
            other => panic!("expected update, got {other:?}"),
        };

        let (inspection, rendered) =
            inspect_and_render_patch_update(Path::new("f.txt"), "top\nstale\nbottom\n", &chunks, "f.txt")
                .await
                .expect("one match drives both views");

        assert_eq!(rendered, "top\nfresh\nbottom\n");
        assert_eq!(inspection.replacements.len(), 1);
        let replacement = &inspection.replacements[0];
        assert_eq!((replacement.start, replacement.removed, replacement.added), (1, 1, 1));
        assert!(!inspection.required_ranges.is_empty());
    }

    #[tokio::test]
    async fn file_loader_and_content_splitter_agree() {
        let dir = tempfile::tempdir().expect("temp dir");
        // Asymmetric inputs: bare `\r`, repeated `\r` before `\n`, mixed
        // endings and an empty document. The on-disk write path and the
        // in-memory preview path must derive the same lines from identical
        // bytes, otherwise a preview/approval can disagree with the write.
        let cases = [
            "plain\n",
            "crlf\r\nline\r\n",
            "a\r",
            "\r",
            "a\r\r\n",
            "a\nb\r\n",
            "",
            "\n\n",
        ];
        for (index, content) in cases.into_iter().enumerate() {
            let path = dir.path().join(format!("split-{index}.txt"));
            tokio::fs::write(&path, content).await.expect("write fixture");
            let from_file = super::load_file_lines(&path).await.expect("load file lines");
            let from_content = super::lines_from_content(content);
            assert_eq!(from_file, from_content, "content {content:?}");
        }
    }

    #[test]
    fn patched_segments_apply_edits_in_file_order() {
        let original: Vec<String> = ["top", "remove", "keep", "bottom"].iter().map(|s| (*s).to_string()).collect();
        let replacements = vec![
            (1usize, 1usize, vec!["replacement".to_string()]),
            (2usize, 0usize, vec!["first-insert".to_string()]),
            (2usize, 0usize, vec!["second-insert".to_string()]),
            (3usize, 1usize, Vec::new()),
        ];

        let plan = plan(original.len(), replacements);
        let segments = patched_line_segments(&original, &plan);
        let lines: Vec<&str> = segments.iter().flat_map(|segment| segment.iter()).map(String::as_str).collect();

        assert_eq!(lines, ["top", "replacement", "first-insert", "second-insert", "keep"]);
    }

    #[test]
    fn plan_constructor_establishes_order_bounds_and_overlap_invariants() {
        let lines = |values: &[&str]| values.iter().map(|value| (*value).to_string()).collect::<Vec<_>>();

        // Out-of-order input is sorted once at construction, not at each render.
        let original = lines(&["a", "b", "c", "d"]);
        let unordered = vec![(2usize, 1usize, lines(&["C"])), (0usize, 1usize, lines(&["A"]))];
        let plan = plan(original.len(), unordered);
        assert_eq!(plan.replacements[0].0, 0);
        assert_eq!(plan.replacements[1].0, 2);

        // Overlapping and out-of-bounds scripts are rejected at construction,
        // before either renderer can silently double-apply or truncate.
        let overlap = ReplacementPlan::build(
            vec![(0usize, 2usize, lines(&["X"])), (1usize, 1usize, lines(&["Y"]))],
            Vec::new(),
            "/fixture",
            original.len(),
        );
        assert!(matches!(overlap, Err(PatchError::OverlappingChunks { .. })), "{overlap:?}");

        let out_of_bounds = ReplacementPlan::build(
            vec![(original.len() + 1, 0usize, lines(&["Z"]))],
            Vec::new(),
            "/fixture",
            original.len(),
        );
        assert!(matches!(out_of_bounds, Err(PatchError::InvalidOperation { .. })), "{out_of_bounds:?}");
    }

    #[test]
    fn trailing_ending_is_a_document_property_not_a_last_line_property() {
        let lines = |values: &[&str]| values.iter().map(|value| (*value).to_string()).collect::<Vec<_>>();
        // Every case is the exact inverse of splitting the expected content
        // into lines: the final line's emptiness must not consume the ending.
        let cases = [
            (lines(&["a"]), true, "a\n"),
            (lines(&["a"]), false, "a"),
            (lines(&["a", ""]), true, "a\n\n"),
            (lines(&["", ""]), true, "\n\n"),
            (lines(&[""]), true, "\n"),
            (Vec::new(), true, "\n"),
        ];
        for (original, ensure_trailing_newline, expected) in cases {
            let plan = plan(original.len(), Vec::new());
            let rendered = render_patched_content(original, &plan, ensure_trailing_newline, LineEnding::LF);
            assert_eq!(rendered, expected, "ensure={ensure_trailing_newline}");
        }
    }

    #[tokio::test]
    async fn streaming_writer_matches_in_memory_renderer() {
        let dir = tempfile::tempdir().expect("temp dir");
        let lines = |values: &[&str]| values.iter().map(|value| (*value).to_string()).collect::<Vec<_>>();
        let scenarios = [
            (lines(&["alpha", "stale", "omega"]), vec![(1usize, 1usize, lines(&["fresh"]))], true),
            (lines(&["", ""]), vec![(0usize, 1usize, lines(&["X"]))], true),
            (lines(&["a"]), vec![(1usize, 0usize, lines(&[""]))], true),
            (lines(&["a", "b"]), vec![(0usize, 1usize, lines(&["c"]))], false),
        ];

        for (index, (original, replacements, ensure_trailing_newline)) in scenarios.into_iter().enumerate() {
            for line_ending in [LineEnding::LF, LineEnding::Crlf] {
                let render_plan = plan(original.len(), replacements.clone());
                let rendered =
                    render_patched_content(original.clone(), &render_plan, ensure_trailing_newline, line_ending);

                let path = dir.path().join(format!("patched-{index}.txt"));
                let mut writer = AtomicWriter::create(&path, None).await.expect("create writer");
                let write_plan = plan(original.len(), replacements.clone());
                write_patched_content(&mut writer, original.clone(), &write_plan, ensure_trailing_newline, line_ending)
                    .await
                    .expect("write patched content");
                writer.commit().await.expect("commit");
                let streamed = tokio::fs::read_to_string(&path).await.expect("read patched file");

                assert_eq!(streamed, rendered, "scenario {index}, {line_ending:?}");
            }
        }
    }
}
