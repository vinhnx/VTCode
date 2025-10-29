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
        return Err(PatchError::InvalidFormat(
            "missing '*** Begin Patch' marker".to_string(),
        ));
    }

    let lines = normalize_patch_lines(raw_lines.as_slice(), true)?;

    let mut operations = Vec::new();
    let mut offset = 1usize; // skip begin marker
    let last = lines.len().saturating_sub(1);
    let mut line_number = 2usize;

    while offset < last {
        if lines[offset].trim().is_empty() {
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

fn normalize_patch_lines<'a>(
    lines: &'a [&'a str],
    lenient: bool,
) -> Result<&'a [&'a str], PatchError> {
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
        (Some(begin), Some(end)) if begin == BEGIN_PATCH_MARKER && end == END_PATCH_MARKER => {
            Ok(())
        }
        (Some(begin), _) if begin != BEGIN_PATCH_MARKER => Err(PatchError::InvalidFormat(
            "missing '*** Begin Patch' marker".to_string(),
        )),
        _ => Err(PatchError::InvalidFormat(
            "missing '*** End Patch' marker".to_string(),
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

fn parse_operation(
    lines: &[&str],
    line_number: usize,
) -> Result<(PatchOperation, usize), PatchError> {
    if lines.is_empty() {
        return Err(invalid_hunk(
            line_number,
            "unexpected end of input before operation header",
        ));
    }

    let header = lines[0].trim();
    if let Some(path) = header.strip_prefix(ADD_FILE_MARKER) {
        parse_add_file(path, &lines[1..])
    } else if let Some(path) = header.strip_prefix(DELETE_FILE_MARKER) {
        parse_delete_file(path)
    } else if let Some(path) = header.strip_prefix(UPDATE_FILE_MARKER) {
        parse_update_file(path, &lines[1..], line_number)
    } else {
        Err(invalid_hunk(
            line_number,
            &format!(
                "invalid hunk header '{header}'. expected '*** Add File', '*** Delete File', or '*** Update File'"
            ),
        ))
    }
}

fn parse_add_file(
    path_text: &str,
    remaining: &[&str],
) -> Result<(PatchOperation, usize), PatchError> {
    let path = path_text.trim();
    validate_patch_path("Add File", path)?;

    let mut content = String::new();
    let mut consumed = 1usize;

    for line in remaining {
        if let Some(body) = line.strip_prefix('+') {
            content.push_str(body);
            content.push('\n');
            consumed += 1;
        } else {
            break;
        }
    }

    Ok((
        PatchOperation::AddFile {
            path: path.to_string(),
            content,
        },
        consumed,
    ))
}

fn parse_delete_file(path_text: &str) -> Result<(PatchOperation, usize), PatchError> {
    let path = path_text.trim();
    validate_patch_path("Delete File", path)?;
    Ok((
        PatchOperation::DeleteFile {
            path: path.to_string(),
        },
        1,
    ))
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

    if let Some(candidate) = remaining
        .get(0)
        .and_then(|line| line.trim().strip_prefix(MOVE_TO_MARKER))
    {
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

        let (chunk, used) = parse_update_chunk(
            &remaining[index..],
            line_number + consumed,
            allow_missing_context,
        )?;
        chunks.push(chunk);
        index += used;
        consumed += used;
        allow_missing_context = false;
    }

    if chunks.is_empty() {
        return Err(invalid_hunk(
            line_number,
            &format!("Update file hunk for path '{path}' is empty"),
        ));
    }

    Ok((
        PatchOperation::UpdateFile {
            path: path.to_string(),
            new_path,
            chunks,
        },
        consumed,
    ))
}

fn parse_update_chunk(
    lines: &[&str],
    line_number: usize,
    allow_missing_context: bool,
) -> Result<(PatchChunk, usize), PatchError> {
    if lines.is_empty() {
        return Err(invalid_hunk(
            line_number,
            "update hunk does not contain any lines",
        ));
    }

    let first = lines[0];
    let (change_context, offset) = if first == EMPTY_CONTEXT_MARKER {
        (None, 1)
    } else if let Some(context) = first.strip_prefix(CONTEXT_MARKER_PREFIX) {
        (Some(context.trim().to_string()), 1)
    } else if allow_missing_context {
        (None, 0)
    } else {
        return Err(invalid_hunk(
            line_number,
            &format!("expected '@@' marker, found '{first}'"),
        ));
    };
    let offset = offset;

    if offset >= lines.len() {
        return Err(invalid_hunk(
            line_number,
            "update hunk does not contain any diff lines",
        ));
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
                return Err(invalid_hunk(
                    line_number,
                    "update hunk does not contain any diff lines",
                ));
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
                chunk
                    .lines
                    .push(PatchLine::Context(current[1..].to_string()));
            }
            Some('+') => {
                chunk
                    .lines
                    .push(PatchLine::Addition(current[1..].to_string()));
            }
            Some('-') => {
                chunk
                    .lines
                    .push(PatchLine::Removal(current[1..].to_string()));
            }
            None => {
                chunk.lines.push(PatchLine::Context(String::new()));
            }
            _ => {
                if parsed_lines == 0 {
                    return Err(invalid_hunk(
                        line_number,
                        &format!("unexpected line '{current}' in update hunk"),
                    ));
                }
                break;
            }
        }

        consumed += 1;
        parsed_lines += 1;
    }

    if parsed_lines == 0 {
        return Err(invalid_hunk(
            line_number,
            "update hunk does not contain any diff lines",
        ));
    }

    Ok((chunk, consumed))
}

fn invalid_hunk(line: usize, message: &str) -> PatchError {
    PatchError::InvalidHunk {
        line,
        message: message.to_string(),
    }
}
