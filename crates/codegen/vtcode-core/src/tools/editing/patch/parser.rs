use super::error::PatchError;
use super::path::validate_patch_path;
use super::{PatchChunk, PatchLine, PatchOperation};

const BEGIN_PATCH_MARKER: &str = "*** Begin Patch";
const END_PATCH_MARKER: &str = "*** End Patch";
const ADD_FILE_MARKER: &str = "*** Add File: ";
const DELETE_FILE_MARKER: &str = "*** Delete File: ";
const UPDATE_FILE_MARKER: &str = "*** Update File: ";
const MOVE_TO_MARKER: &str = "*** Move to: ";
const EOF_MARKER: &str = "*** End of File";
const EMPTY_CONTEXT_MARKER: &str = "@@";
const CONTEXT_MARKER_PREFIX: &str = "@@ ";

pub(crate) fn parse(input: &str) -> Result<Vec<PatchOperation>, PatchError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(PatchError::EmptyInput);
    }

    let raw_lines: Vec<&str> = trimmed.lines().collect();
    if raw_lines.is_empty() {
        return Err(PatchError::InvalidFormat("missing '*** Begin Patch' marker".to_string()));
    }

    let lines = normalize_patch_lines(raw_lines.as_slice(), true)?;

    let mut operations = Vec::new();
    let mut offset = 1usize; // skip begin marker
    let last = lines.len().saturating_sub(1);
    let mut line_number = 2usize;

    while offset < last {
        let trimmed = lines[offset].trim();
        if trimmed.is_empty() || trimmed == END_PATCH_MARKER {
            // Tolerate stray `*** End Patch` markers: models often duplicate
            // the envelope terminator (observed: trailing `*** End Patch`
            // twice fails with "invalid hunk header '*** End Patch'"). The
            // authoritative terminator is the final line, excluded via `last`,
            // so interior duplicates carry no information.
            offset += 1;
            line_number += 1;
            continue;
        }

        let (operation, consumed) = parse_operation(&lines[offset..last], line_number)?;
        operations.push(operation);
        offset += consumed;
        line_number += consumed;
    }

    Ok(operations)
}

fn normalize_patch_lines<'a>(lines: &'a [&'a str], lenient: bool) -> Result<&'a [&'a str], PatchError> {
    match check_patch_boundaries(lines) {
        Ok(()) => Ok(lines),
        Err(err) => {
            if lenient {
                if let Some(inner) = strip_heredoc(lines) {
                    check_patch_boundaries(inner)?;
                    Ok(inner)
                } else {
                    Err(err)
                }
            } else {
                Err(err)
            }
        }
    }
}

fn check_patch_boundaries(lines: &[&str]) -> Result<(), PatchError> {
    let first = lines.first().copied().map(str::trim);
    let last = lines.last().copied().map(str::trim);

    match (first, last) {
        (Some(begin), Some(end)) if begin == BEGIN_PATCH_MARKER && end == END_PATCH_MARKER => Ok(()),
        (Some(_begin), _) if first != Some(BEGIN_PATCH_MARKER) => {
            // The first line is not the `*** Begin Patch` envelope header.
            // Detect the two most common mis-formats and emit an actionable,
            // one-shot recovery hint so the model does not waste a round-trip
            // guessing (see checkpoint turn_615, where a unified diff in the
            // `patch` field produced a bare "missing marker" error).
            let joined = lines.join("\n");
            let hint = if super::looks_like_unified_diff(&joined) {
                "input looks like a standard unified diff (---/+++ format), \
                 but apply_patch requires the VT Code envelope. Reformat as: \
                 *** Begin Patch / *** Update File: <path> / @@ <optional context> \
                 / -old line / +new line / *** End Patch"
            } else if !super::looks_like_vte_patch(&joined) {
                "input contains no VT Code patch markers (*** Begin Patch / \
                 *** Update File: / *** Add File:). apply_patch expects a patch \
                 envelope, not raw file contents. Wrap your change in: \
                 *** Begin Patch ... *** End Patch"
            } else {
                "missing '*** Begin Patch' marker"
            };
            Err(PatchError::InvalidFormat(hint.to_string()))
        }
        _ => Err(PatchError::InvalidFormat(
            "missing '*** End Patch' marker. End every patch with `*** End Patch` on its own line; \
             for large new files prefer `write_file` or split into smaller `*** Add File`/`*** Update File` chunks"
                .to_string(),
        )),
    }
}

fn strip_heredoc<'a>(lines: &'a [&'a str]) -> Option<&'a [&'a str]> {
    if lines.len() < 4 {
        return None;
    }

    let first = lines.first()?.trim();
    let last = lines.last()?.trim();

    if (first == "<<EOF" || first == "<<'EOF'" || first == "<<\"EOF\"") && last.ends_with("EOF") {
        Some(&lines[1..lines.len() - 1])
    } else {
        None
    }
}

fn parse_operation(lines: &[&str], line_number: usize) -> Result<(PatchOperation, usize), PatchError> {
    if lines.is_empty() {
        return Err(invalid_hunk(line_number, "unexpected end of input before operation header"));
    }

    let header = lines[0].trim();
    if let Some(path) = header.strip_prefix(ADD_FILE_MARKER) {
        parse_add_file(path, &lines[1..])
    } else if let Some(path) = header.strip_prefix(DELETE_FILE_MARKER) {
        parse_delete_file(path)
    } else if let Some(path) = header.strip_prefix(UPDATE_FILE_MARKER) {
        parse_update_file(path, &lines[1..], line_number)
    } else if header.starts_with("@@") {
        Err(invalid_hunk(
            line_number,
            "stray '@@' line outside an update hunk. '@@' context markers belong inside \
             '*** Update File' hunks (a single '@@' directly after '*** Add File:' is skipped); \
             start a new operation with '*** Add File:', '*** Delete File:', or '*** Update File:'",
        ))
    } else {
        Err(invalid_hunk(
            line_number,
            &format!(
                "invalid hunk header '{header}'. expected '*** Add File', '*** Delete File', or '*** Update File'. Unified diff format (---/+++ style) is not accepted. VT Code patch format example:\n*** Begin Patch\n*** Update File: path/to/file.rs\n@@ optional context\n-old line\n+new line\n*** End Patch"
            ),
        ))
    }
}

fn parse_add_file(path_text: &str, remaining: &[&str]) -> Result<(PatchOperation, usize), PatchError> {
    let path = path_text.trim();
    validate_patch_path("Add File", path)?;

    // Tolerate a single Update-shaped `@@` context line directly after the
    // header: models often copy the `*** Update File` shape (`@@`, then `+`
    // lines) after `*** Add File:`. A bare `@@` line can never be Add File
    // content (content lines carry a `+` prefix; `+@@ ...` still parses as
    // content), so skipping it only turns a hard parse error into the file
    // the caller intended. Observed in live sessions (Nov 2026 README task).
    let skip_marker = matches!(
        remaining.first().map(|line| line.trim()),
        Some(marker) if marker == EMPTY_CONTEXT_MARKER || marker.starts_with(CONTEXT_MARKER_PREFIX)
    );
    let skipped = usize::from(skip_marker);

    let mut content = String::new();
    let mut consumed = 1usize + skipped;

    for line in remaining.iter().skip(skipped) {
        if let Some(body) = line.strip_prefix('+') {
            content.push_str(body);
            content.push('\n');
            consumed += 1;
        } else {
            break;
        }
    }

    Ok((PatchOperation::AddFile { path: path.to_string(), content }, consumed))
}

fn parse_delete_file(path_text: &str) -> Result<(PatchOperation, usize), PatchError> {
    let path = path_text.trim();
    validate_patch_path("Delete File", path)?;
    Ok((PatchOperation::DeleteFile { path: path.to_string() }, 1))
}

fn parse_update_file(
    path_text: &str,
    remaining: &[&str],
    line_number: usize,
) -> Result<(PatchOperation, usize), PatchError> {
    let path = path_text.trim();
    validate_patch_path("Update File", path)?;

    let mut consumed = 1usize;
    let mut index = 0usize;
    let mut new_path = None;

    if let Some(candidate) = remaining.first().and_then(|line| line.trim().strip_prefix(MOVE_TO_MARKER)) {
        let candidate_trimmed = candidate.trim();
        validate_patch_path("Move to", candidate_trimmed)?;
        new_path = Some(candidate_trimmed.to_string());
        index += 1;
        consumed += 1;
    }

    let mut chunks = Vec::new();
    let mut allow_missing_context = true;

    while index < remaining.len() {
        let next_line = remaining[index].trim();
        if next_line.starts_with("***") && next_line != EOF_MARKER {
            break;
        }

        if next_line.is_empty() {
            index += 1;
            consumed += 1;
            continue;
        }

        let (chunk, used) = parse_update_chunk(&remaining[index..], line_number + consumed, allow_missing_context)?;
        chunks.push(chunk);
        index += used;
        consumed += used;
        allow_missing_context = false;
    }

    if chunks.is_empty() {
        return Err(invalid_hunk(line_number, &format!("Update file hunk for path '{path}' is empty")));
    }

    Ok((PatchOperation::UpdateFile { path: path.to_string(), new_path, chunks }, consumed))
}

fn parse_update_chunk(
    lines: &[&str],
    line_number: usize,
    allow_missing_context: bool,
) -> Result<(PatchChunk, usize), PatchError> {
    if lines.is_empty() {
        return Err(invalid_hunk(line_number, "update hunk does not contain any lines"));
    }

    let first = lines[0];
    let (change_context, offset) = if first == EMPTY_CONTEXT_MARKER {
        (None, 1)
    } else if let Some(context) = first.strip_prefix(CONTEXT_MARKER_PREFIX) {
        let context = context.trim();
        // Strip trailing "@@" delimiter if present (common in unified-diff-style anchors).
        let context = context.strip_suffix("@@").unwrap_or(context).trim();
        (Some(context.to_owned()), 1)
    } else if allow_missing_context {
        (None, 0)
    } else {
        return Err(invalid_hunk(line_number, &format!("expected '@@' marker, found '{first}'")));
    };

    if offset >= lines.len() {
        return Err(invalid_hunk(line_number, "update hunk does not contain any diff lines"));
    }

    let mut chunk = PatchChunk {
        change_context,
        lines: Vec::new(),
        is_end_of_file: false,
    };

    let mut consumed = offset;
    let mut parsed_lines = 0usize;

    while consumed < lines.len() {
        let current = lines[consumed];
        if current == EOF_MARKER {
            if parsed_lines == 0 {
                return Err(invalid_hunk(line_number, "update hunk does not contain any diff lines"));
            }
            chunk.is_end_of_file = true;
            consumed += 1;
            break;
        }

        if current.starts_with("*** ") {
            break;
        }

        if current.starts_with("@@") && parsed_lines > 0 {
            break;
        }

        match current.chars().next() {
            Some(' ') => {
                chunk.lines.push(PatchLine::Context(current[1..].to_string()));
            }
            Some('+') => {
                chunk.lines.push(PatchLine::Addition(current[1..].to_string()));
            }
            Some('-') => {
                chunk.lines.push(PatchLine::Removal(current[1..].to_string()));
            }
            None => {
                chunk.lines.push(PatchLine::Context(String::new()));
            }
            _ => {
                if parsed_lines == 0 {
                    return Err(invalid_hunk(line_number, &format!("unexpected line '{current}' in update hunk")));
                }
                break;
            }
        }

        consumed += 1;
        parsed_lines += 1;
    }

    if parsed_lines == 0 {
        return Err(invalid_hunk(line_number, "update hunk does not contain any diff lines"));
    }

    Ok((chunk, consumed))
}

fn invalid_hunk(line: usize, message: &str) -> PatchError {
    PatchError::InvalidHunk { line, message: message.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_strips_trailing_at_at() {
        let operations =
            parse("*** Begin Patch\n*** Update File: README.md\n@@ section @@\n+line\n*** End Patch").unwrap();
        match &operations[0] {
            PatchOperation::UpdateFile { chunks, .. } => {
                assert_eq!(
                    chunks[0].change_context.as_deref(),
                    Some("section"),
                    "trailing @@ should be stripped from context"
                );
            }
            other => panic!("expected UpdateFile, got {other:?}"),
        }
    }

    #[test]
    fn context_without_trailing_at_at() {
        let operations =
            parse("*** Begin Patch\n*** Update File: README.md\n@@ section\n+line\n*** End Patch").unwrap();
        match &operations[0] {
            PatchOperation::UpdateFile { chunks, .. } => {
                assert_eq!(chunks[0].change_context.as_deref(), Some("section"));
            }
            other => panic!("expected UpdateFile, got {other:?}"),
        }
    }

    #[test]
    fn empty_context_marker() {
        let operations = parse("*** Begin Patch\n*** Update File: README.md\n@@\n+line\n*** End Patch").unwrap();
        match &operations[0] {
            PatchOperation::UpdateFile { chunks, .. } => {
                assert_eq!(chunks[0].change_context, None);
            }
            other => panic!("expected UpdateFile, got {other:?}"),
        }
    }

    #[test]
    fn context_with_multiple_trailing_at_at() {
        let operations =
            parse("*** Begin Patch\n*** Update File: f.txt\n@@ foo @@ bar @@\n+line\n*** End Patch").unwrap();
        match &operations[0] {
            PatchOperation::UpdateFile { chunks, .. } => {
                // Only the final "@@" should be stripped
                assert_eq!(
                    chunks[0].change_context.as_deref(),
                    Some("foo @@ bar"),
                    "only trailing @@ should be stripped"
                );
            }
            other => panic!("expected UpdateFile, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Actionable boundary-check errors (see checkpoint turn_615)
    // -----------------------------------------------------------------------

    #[test]
    fn unified_diff_input_produces_actionable_error() {
        let diff = "diff --git a/src/main.rs b/src/main.rs\n\
                    index abc..def 100644\n\
                    --- a/src/main.rs\n\
                    +++ b/src/main.rs\n\
                    @@ -1,3 +1,3 @@\n\
                    -old\n\
                    +new\n";
        let err = parse(diff).expect_err("unified diff should be rejected");
        let msg = err.to_string();
        assert!(msg.contains("unified diff"), "error should identify the format: {msg}");
        assert!(msg.contains("*** Begin Patch"), "error should name the required envelope: {msg}");
    }

    #[test]
    fn raw_source_input_produces_envelope_hint() {
        // Raw file contents (no patch markers at all) should get a distinct
        // hint telling the model to wrap the change in the patch envelope.
        let source = "pub(crate) async fn install_update(&self, force: bool) {\n\
                      self.install_update_with_progress(force, true).await\n\
                      }\n";
        let err = parse(source).expect_err("raw source should be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("envelope") || msg.contains("*** Begin Patch"),
            "error should mention the envelope: {msg}"
        );
        assert!(!msg.contains("unified diff"), "raw source should not be confused with a unified diff: {msg}");
    }

    #[test]
    fn valid_envelope_still_parses_after_hint_change() {
        // Regression guard: the boundary-check enhancement must not break
        // well-formed patches.
        let ops = parse("*** Begin Patch\n*** Add File: f.txt\n+hi\n*** End Patch").expect("valid patch should parse");
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn duplicated_trailing_end_patch_marker_is_tolerated() {
        // Observed in live sessions: the model emits `*** End Patch` twice,
        // which previously failed with "invalid hunk header '*** End Patch'".
        let ops = parse(
            "*** Begin Patch\n*** Update File: crates/codegen/vtcode-core/src/cli/man_pages.rs\n@@\n+line\n*** End Patch\n*** End Patch",
        )
        .expect("duplicated end marker should parse");
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn stray_end_patch_between_operations_is_tolerated() {
        let ops = parse(
            "*** Begin Patch\n*** Add File: a.txt\n+hi\n*** End Patch\n*** Update File: b.txt\n@@\n+yo\n*** End Patch",
        )
        .expect("interior end marker should parse");
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn add_file_with_bare_context_marker_is_tolerated() {
        // Observed in live sessions: the model copies the Update shape (`@@`,
        // then `+` lines) after `*** Add File:`, which previously failed with
        // "invalid hunk header '@@'" on the `@@` line.
        let ops = parse("*** Begin Patch\n*** Add File: f.txt\n@@\n+hello\n+world\n*** End Patch")
            .expect("bare @@ after Add File should be skipped");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PatchOperation::AddFile { path, content } => {
                assert_eq!(path, "f.txt");
                assert_eq!(content, "hello\nworld\n", "marker must not leak into content");
            }
            other => panic!("expected AddFile, got {other:?}"),
        }
    }

    #[test]
    fn add_file_with_context_text_marker_is_tolerated() {
        let ops = parse("*** Begin Patch\n*** Add File: f.txt\n@@ new file\n+hello\n*** End Patch")
            .expect("@@ with context after Add File should be skipped");
        match &ops[0] {
            PatchOperation::AddFile { content, .. } => assert_eq!(content, "hello\n"),
            other => panic!("expected AddFile, got {other:?}"),
        }
    }

    #[test]
    fn add_file_prefixed_context_marker_stays_content() {
        // Asymmetry: `+@@ ...` IS content (it carries the `+` prefix); only a
        // bare `@@` line is skipped.
        let ops = parse("*** Begin Patch\n*** Add File: f.txt\n+@@ not a marker\n*** End Patch")
            .expect("prefixed @@ is content");
        match &ops[0] {
            PatchOperation::AddFile { content, .. } => assert_eq!(content, "@@ not a marker\n"),
            other => panic!("expected AddFile, got {other:?}"),
        }
    }

    #[test]
    fn second_context_marker_after_add_file_still_fails_actionably() {
        // Only ONE leading `@@` is skipped; a second one is stray and must
        // fail with the targeted message, not the generic hunk-header text.
        let err = parse("*** Begin Patch\n*** Add File: f.txt\n@@\n@@\n+hi\n*** End Patch")
            .expect_err("second @@ should still fail");
        let msg = err.to_string();
        assert!(msg.contains("stray '@@'"), "error should name the stray marker: {msg}");
        assert!(msg.contains("Update File"), "error should point at update hunks: {msg}");
    }

    #[test]
    fn stray_context_marker_between_operations_fails_actionably() {
        let err =
            parse("*** Begin Patch\n*** Delete File: gone.txt\n@@\n*** Update File: f.txt\n@@\n+hi\n*** End Patch")
                .expect_err("stray @@ should fail");
        let msg = err.to_string();
        assert!(msg.contains("stray '@@'"), "error should name the stray marker: {msg}");
        assert!(msg.contains("Update File"), "error should point at update hunks: {msg}");
    }
}
