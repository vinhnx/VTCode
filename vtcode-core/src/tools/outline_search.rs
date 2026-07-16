//! Bounded, read-only declaration discovery for `code_search`.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::tools::ast_grep_installer::AstGrepStatus;
use crate::tools::ast_grep_language::AstGrepLanguage;
use vtcode_commons::exclusions::is_sensitive_file;
use vtcode_commons::walk::{build_walker_single_threaded, is_excluded_dir};
const CODE_SEARCH_OUTLINE_BYTE_CAP: usize = 1024 * 1024;
const CODE_SEARCH_OUTLINE_PATH_BATCH_SIZE: usize = 64;
const CODE_SEARCH_OUTLINE_ARG_BATCH_BYTE_CAP: usize = 16 * 1024;
const CODE_SEARCH_OUTLINE_FIXED_ARGS: [&str; 4] = ["outline", "--json=stream", "--items", "all"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclarationRange {
    pub byte_start: usize,
    pub byte_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclarationRecord {
    pub name: String,
    pub range: DeclarationRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclarationFileRecord {
    pub path: PathBuf,
    pub language: AstGrepLanguage,
    pub declarations: Vec<DeclarationRecord>,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclarationSearchOutcome {
    pub files: Vec<DeclarationFileRecord>,
    pub stream_complete: bool,
    pub truncated: bool,
}

async fn kill_and_reap_declaration_child(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundedRecordRead {
    Record,
    Eof,
    Exhausted,
}

async fn read_bounded_record<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    record: &mut Vec<u8>,
    bytes_read: &mut usize,
    byte_cap: usize,
) -> std::io::Result<BoundedRecordRead> {
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if record.is_empty() {
                BoundedRecordRead::Eof
            } else {
                BoundedRecordRead::Record
            });
        }
        if *bytes_read >= byte_cap {
            return Ok(BoundedRecordRead::Exhausted);
        }

        let remaining = byte_cap - *bytes_read;
        let bounded = &available[..available.len().min(remaining)];
        let consumed = bounded
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bounded.len(), |index| index + 1);
        let record_complete = bounded.get(consumed.saturating_sub(1)) == Some(&b'\n');
        record.extend_from_slice(&bounded[..consumed]);
        reader.consume(consumed);
        *bytes_read += consumed;
        if record_complete {
            return Ok(BoundedRecordRead::Record);
        }
    }
}

fn smart_case_eq(left: &str, query: &str) -> bool {
    if query.chars().any(char::is_uppercase) {
        left == query
    } else {
        left.to_lowercase() == query.to_lowercase()
    }
}

fn matching_declarations(
    file: &OutlineFile,
    query: &str,
    candidate_cap: usize,
) -> (Vec<DeclarationRecord>, bool) {
    let matching = file
        .items
        .iter()
        .filter(|item| !item.is_import)
        .flat_map(|item| {
            std::iter::once((&item.name, &item.range)).chain(
                item.members
                    .iter()
                    .map(|member| (&member.name, &member.range)),
            )
        })
        .filter(|(name, _)| smart_case_eq(name, query))
        .collect::<Vec<_>>();
    let matching_count = matching.len();
    let records = matching
        .into_iter()
        .filter_map(|(name, range)| {
            let range = range.as_ref()?;
            Some(DeclarationRecord {
                name: name.clone(),
                range: DeclarationRange {
                    byte_start: usize::try_from(range.byte_offset.start).ok()?,
                    byte_end: usize::try_from(range.byte_offset.end).ok()?,
                },
            })
        })
        .collect::<Vec<_>>();
    let complete = records.len() == matching_count && records.len() <= candidate_cap;
    (records.into_iter().take(candidate_cap).collect(), complete)
}

/// Stream recognised declarations using only an already installed outline
/// executable. No installation or cache mutation is attempted.
pub(crate) async fn search_declarations_bounded(
    workspace_root: &Path,
    resolved_path: &Path,
    query: &str,
    languages: &[AstGrepLanguage],
    candidate_cap: usize,
) -> Result<DeclarationSearchOutcome> {
    let binary = match AstGrepStatus::check() {
        AstGrepStatus::Available { binary, .. } => binary,
        AstGrepStatus::NotFound => bail!("definition search is unavailable"),
        AstGrepStatus::Error { .. } => bail!("definition search is unavailable"),
    };
    let mut command_paths = outline_paths(workspace_root, resolved_path, languages).peekable();
    let mut bytes_read = 0usize;
    let mut retained = 0usize;
    let mut files = Vec::new();
    let mut truncated = false;
    while command_paths.peek().is_some() && !truncated {
        let command_args = next_outline_path_batch(&binary, &mut command_paths)?;
        let mut command = Command::new(&binary);
        command
            .current_dir(workspace_root)
            .env("RAYON_NUM_THREADS", "1")
            .args(CODE_SEARCH_OUTLINE_FIXED_ARGS)
            .args(command_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = command.spawn().context("failed to run definition search")?;
        let Some(stdout) = child.stdout.take() else {
            kill_and_reap_declaration_child(&mut child).await;
            bail!("failed to capture definition search output");
        };
        let mut reader = BufReader::new(stdout);
        let mut line_buf = Vec::new();

        loop {
            line_buf.clear();
            let read = match read_bounded_record(
                &mut reader,
                &mut line_buf,
                &mut bytes_read,
                CODE_SEARCH_OUTLINE_BYTE_CAP,
            )
            .await
            {
                Ok(read) => read,
                Err(error) => {
                    drop(reader);
                    kill_and_reap_declaration_child(&mut child).await;
                    return Err(error).context("failed to read definition stream");
                }
            };
            match read {
                BoundedRecordRead::Record => {}
                BoundedRecordRead::Eof => break,
                BoundedRecordRead::Exhausted => {
                    truncated = true;
                    break;
                }
            }
            let file = match serde_json::from_slice::<OutlineFile>(&line_buf) {
                Ok(file) => file,
                Err(error) => {
                    drop(reader);
                    kill_and_reap_declaration_child(&mut child).await;
                    return Err(error).context("failed to parse definition stream record");
                }
            };
            let path = PathBuf::from(&file.path);
            let Some(language) = AstGrepLanguage::from_path(&path)
                .or_else(|| AstGrepLanguage::from_user_value(&file.lang))
            else {
                continue;
            };
            if !languages.is_empty() && !languages.contains(&language) {
                continue;
            }
            let remaining = candidate_cap.saturating_sub(retained);
            let (declarations, complete) = matching_declarations(&file, query, remaining);
            retained = retained.saturating_add(declarations.len());
            files.push(DeclarationFileRecord {
                path,
                language,
                declarations,
                complete,
            });
            if !complete || retained >= candidate_cap {
                truncated = true;
                break;
            }
        }

        drop(reader);
        if truncated {
            let _ = child.start_kill();
        }
        let status = child
            .wait()
            .await
            .context("failed to reap definition search process")?;
        if !truncated && !status.success() {
            bail!("definition search failed");
        }
    }

    Ok(DeclarationSearchOutcome {
        files,
        stream_complete: !truncated,
        truncated,
    })
}

fn outline_paths<'a>(
    workspace_root: &'a Path,
    resolved_path: &'a Path,
    languages: &'a [AstGrepLanguage],
) -> Box<dyn Iterator<Item = String> + Send + 'a> {
    if resolved_path.is_file() {
        return Box::new(std::iter::once(command_path_arg(
            workspace_root,
            resolved_path,
        )));
    }
    let mut builder = build_walker_single_threaded(resolved_path);
    builder.filter_entry(|entry| !is_excluded_dir(entry));
    builder.sort_by_file_path(|left, right| left.cmp(right));
    let paths = builder
        .build()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(|entry| entry.into_path())
        .filter(|path| {
            !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_sensitive_file)
                && (languages.is_empty()
                    || AstGrepLanguage::from_path(path)
                        .is_some_and(|language| languages.contains(&language)))
        })
        .map(|path| command_path_arg(workspace_root, &path));
    Box::new(paths)
}

fn next_outline_path_batch(
    executable: &Path,
    paths: &mut std::iter::Peekable<impl Iterator<Item = String>>,
) -> Result<Vec<String>> {
    next_outline_path_batch_with_cap(executable, paths, CODE_SEARCH_OUTLINE_ARG_BATCH_BYTE_CAP)
}

fn next_outline_path_batch_with_cap(
    executable: &Path,
    paths: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    arg_byte_cap: usize,
) -> Result<Vec<String>> {
    let mut batch = Vec::with_capacity(CODE_SEARCH_OUTLINE_PATH_BATCH_SIZE);
    let mut total_arg_bytes = arg_os_bytes(executable.as_os_str());
    total_arg_bytes = CODE_SEARCH_OUTLINE_FIXED_ARGS
        .iter()
        .fold(total_arg_bytes, |total, arg| {
            total.saturating_add(arg_bytes(arg))
        });
    while batch.len() < CODE_SEARCH_OUTLINE_PATH_BATCH_SIZE {
        let Some(next) = paths.peek() else {
            break;
        };
        let next_bytes = arg_bytes(next);
        if total_arg_bytes.saturating_add(next_bytes) > arg_byte_cap {
            if batch.is_empty() {
                bail!("definition search path exceeds the command argument byte limit");
            }
            break;
        }
        let Some(next) = paths.next() else {
            break;
        };
        total_arg_bytes = total_arg_bytes.saturating_add(next_bytes);
        batch.push(next);
    }
    Ok(batch)
}

fn arg_bytes(arg: &str) -> usize {
    arg.as_bytes().len().saturating_add(1)
}

fn arg_os_bytes(arg: &std::ffi::OsStr) -> usize {
    arg.as_encoded_bytes().len().saturating_add(1)
}

/// Build the path argument passed to ast-grep. Use the workspace-relative form
/// when possible so the emitted `path` field is relative and readable.
fn command_path_arg(workspace_root: &Path, resolved: &Path) -> String {
    let workspace_canonical =
        std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
    if let Ok(relative) = resolved.strip_prefix(&workspace_canonical) {
        if relative.as_os_str().is_empty() {
            ".".to_string()
        } else {
            relative.to_string_lossy().replace('\\', "/")
        }
    } else {
        resolved.to_string_lossy().to_string()
    }
}

// ---------------------------------------------------------------------------
// Tolerant deserialization of the ast-grep outline JSON stream.
//
// The outline JSON is an alpha preview surface in ast-grep 0.44.0. Unknown
// keys may appear in future versions, so every struct uses `#[serde(default)]`
// and none set `deny_unknown_fields`. Missing fields degrade gracefully
// instead of failing the whole call.
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Deserialize)]
struct OutlineFile {
    #[serde(default)]
    path: String,
    #[serde(default, rename = "language")]
    lang: String,
    #[serde(default)]
    items: Vec<OutlineItem>,
}

/// Source range as reported by ast-grep outline. All line/column values are
/// zero-based in the raw stream. We expose the raw range plus a derived
/// 1-based `lineRange` in the `full` view so callers can feed the lines
/// straight to `file_operation` `read` (`offset_lines` is 1-based, inclusive).
///
/// Forward-compat tolerant: `#[serde(default)]` and no `deny_unknown_fields`,
/// so unknown keys from future ast-grep versions are ignored instead of
/// failing the whole call.
#[derive(Debug, Default, Clone, Deserialize)]
struct OutlineRange {
    #[serde(default, rename = "byteOffset")]
    byte_offset: OutlineByteOffset,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct OutlineByteOffset {
    #[serde(default)]
    start: u64,
    #[serde(default)]
    end: u64,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct OutlineItem {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "isImport")]
    is_import: bool,
    #[serde(default)]
    range: Option<OutlineRange>,
    #[serde(default)]
    members: Vec<OutlineMember>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct OutlineMember {
    #[serde(default)]
    name: String,
    #[serde(default)]
    range: Option<OutlineRange>,
}

#[cfg(test)]
mod tests;
