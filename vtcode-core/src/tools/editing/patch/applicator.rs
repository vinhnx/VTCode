use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::fs;
use tokio::io::AsyncWriteExt;

use super::error::PatchError;
use super::matcher::PatchContextMatcher;
use super::{PatchChunk, PatchOperation};

pub(crate) async fn apply(
    root: &Path,
    operations: &[PatchOperation],
) -> Result<Vec<String>, PatchError> {
    let mut results = Vec::new();

    for operation in operations {
        match operation {
            PatchOperation::AddFile { path, content } => {
                let full_path = root.join(path);
                write_atomic(&full_path, content.as_bytes()).await?;
                results.push(format!("Added file: {path}"));
            }
            PatchOperation::DeleteFile { path } => {
                let full_path = root.join(path);
                match fs::metadata(&full_path).await {
                    Ok(metadata) => {
                        if metadata.is_dir() {
                            fs::remove_dir_all(&full_path)
                                .await
                                .map_err(|err| PatchError::Io {
                                    action: "delete",
                                    path: full_path.clone(),
                                    source: err,
                                })?;
                        } else {
                            fs::remove_file(&full_path)
                                .await
                                .map_err(|err| PatchError::Io {
                                    action: "delete",
                                    path: full_path.clone(),
                                    source: err,
                                })?;
                        }
                        results.push(format!("Deleted file: {path}"));
                    }
                    Err(err) if err.kind() == ErrorKind::NotFound => {
                        results.push(format!("File not found, skipped deletion: {path}"));
                    }
                    Err(err) => {
                        return Err(PatchError::Io {
                            action: "inspect",
                            path: full_path,
                            source: err,
                        });
                    }
                }
            }
            PatchOperation::UpdateFile {
                path,
                new_path,
                chunks,
            } => {
                let source_path = root.join(path);
                let existing =
                    fs::read_to_string(&source_path)
                        .await
                        .map_err(|err| PatchError::Io {
                            action: "read",
                            path: source_path.clone(),
                            source: err,
                        })?;

                let new_content = compute_new_content(&existing, path, chunks)?;

                match new_path {
                    Some(dest_rel) => {
                        let dest_path = root.join(dest_rel);
                        write_atomic(&dest_path, new_content.as_bytes()).await?;

                        if dest_path != source_path {
                            match fs::remove_file(&source_path).await {
                                Ok(()) => {}
                                Err(err) if err.kind() == ErrorKind::NotFound => {}
                                Err(err) => {
                                    return Err(PatchError::Io {
                                        action: "delete",
                                        path: source_path,
                                        source: err,
                                    });
                                }
                            }
                        }

                        results.push(format!("Updated file: {path} -> {dest_rel}"));
                    }
                    None => {
                        write_atomic(&source_path, new_content.as_bytes()).await?;
                        results.push(format!("Updated file: {path}"));
                    }
                }
            }
        }
    }

    Ok(results)
}

fn compute_new_content(
    existing: &str,
    path: &str,
    chunks: &[PatchChunk],
) -> Result<String, PatchError> {
    let mut original_lines: Vec<String> =
        existing.split('\n').map(|line| line.to_string()).collect();
    let had_trailing_newline = existing.ends_with('\n');

    if had_trailing_newline && original_lines.last().is_some_and(|line| line.is_empty()) {
        original_lines.pop();
    }

    let replacements = compute_replacements(&original_lines, chunks, path)?;
    let mut new_lines = apply_replacements(original_lines, &replacements);

    if had_trailing_newline || chunks.iter().any(|chunk| chunk.is_end_of_file()) {
        if !new_lines.last().is_some_and(|line| line.is_empty()) {
            new_lines.push(String::new());
        }
    }

    Ok(new_lines.join("\n"))
}

fn compute_replacements(
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

async fn write_atomic(path: &Path, content: &[u8]) -> Result<(), PatchError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|err| PatchError::Io {
                action: "create directories",
                path: parent.to_path_buf(),
                source: err,
            })?;
    }

    let temp_path = temporary_path(path)?;
    let mut file = fs::File::create(&temp_path)
        .await
        .map_err(|err| PatchError::Io {
            action: "create",
            path: temp_path.clone(),
            source: err,
        })?;

    file.write_all(content)
        .await
        .map_err(|err| PatchError::Io {
            action: "write",
            path: temp_path.clone(),
            source: err,
        })?;
    file.flush().await.map_err(|err| PatchError::Io {
        action: "flush",
        path: temp_path.clone(),
        source: err,
    })?;
    drop(file);

    match fs::rename(&temp_path, path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            fs::remove_file(path)
                .await
                .map_err(|del_err| PatchError::Io {
                    action: "delete",
                    path: path.to_path_buf(),
                    source: del_err,
                })?;
            fs::rename(&temp_path, path)
                .await
                .map_err(|rename_err| PatchError::Io {
                    action: "rename",
                    path: path.to_path_buf(),
                    source: rename_err,
                })
        }
        Err(err) => {
            let _ = fs::remove_file(&temp_path).await;
            Err(PatchError::Io {
                action: "rename",
                path: path.to_path_buf(),
                source: err,
            })
        }
    }
}

fn temporary_path(target: &Path) -> Result<PathBuf, PatchError> {
    let parent = target
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("vtcode-patch");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| PatchError::TempPath {
            path: target.to_path_buf(),
            source: err,
        })?
        .as_nanos();
    let pid = std::process::id();
    let temp_name = format!(".{file_name}.{pid}.{timestamp}.tmp");
    Ok(parent.join(temp_name))
}
