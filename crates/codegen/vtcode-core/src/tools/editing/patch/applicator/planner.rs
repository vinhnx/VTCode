use std::fs::Permissions;
use std::io::ErrorKind;
use std::path::Path;

use tokio::fs;

use super::{PatchChunk, PatchError, PatchOperation};

pub(crate) enum PreparedOperation<'a> {
    Add {
        path: &'a str,
        content: &'a str,
    },
    Delete {
        path: &'a str,
    },
    Update {
        path: &'a str,
        new_path: Option<&'a str>,
        chunks: &'a [PatchChunk],
        permissions: Permissions,
    },
}

pub(crate) async fn plan_operations<'a>(
    root: &Path,
    operations: &'a [PatchOperation],
) -> Result<Vec<PreparedOperation<'a>>, PatchError> {
    if operations.is_empty() {
        return Err(PatchError::NoOperations);
    }

    let mut prepared = Vec::with_capacity(operations.len());

    for operation in operations {
        match operation {
            PatchOperation::AddFile { path, content } => {
                let full_path = root.join(path);
                match fs::metadata(&full_path).await {
                    Ok(_) => {
                        return Err(PatchError::InvalidOperation {
                            path: path.clone(),
                            reason: "target already exists".to_string(),
                        });
                    }
                    Err(err) if err.kind() == ErrorKind::NotFound => {
                        prepared.push(PreparedOperation::Add { path, content });
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
            PatchOperation::DeleteFile { path } => {
                prepared.push(PreparedOperation::Delete { path });
            }
            PatchOperation::UpdateFile {
                path,
                new_path,
                chunks,
            } => {
                let source_path = root.join(path);
                let metadata = fs::metadata(&source_path).await.map_err(|err| {
                    if err.kind() == ErrorKind::NotFound {
                        PatchError::MissingFile { path: path.clone() }
                    } else {
                        PatchError::Io {
                            action: "inspect",
                            path: source_path.clone(),
                            source: err,
                        }
                    }
                })?;

                if metadata.is_dir() {
                    return Err(PatchError::InvalidOperation {
                        path: path.clone(),
                        reason: "cannot apply text diff to directory".to_string(),
                    });
                }

                if let Some(dest_rel) = new_path
                    .as_ref()
                    .filter(|candidate| candidate.as_str() != path)
                {
                    let destination = root.join(dest_rel);
                    match fs::metadata(&destination).await {
                        Ok(existing) => {
                            let reason = if existing.is_dir() {
                                "destination is a directory"
                            } else {
                                "destination file already exists"
                            };
                            return Err(PatchError::InvalidOperation {
                                path: dest_rel.clone(),
                                reason: reason.to_string(),
                            });
                        }
                        Err(err) if err.kind() == ErrorKind::NotFound => {}
                        Err(err) => {
                            return Err(PatchError::Io {
                                action: "inspect",
                                path: destination,
                                source: err,
                            });
                        }
                    }
                }

                prepared.push(PreparedOperation::Update {
                    path,
                    new_path: new_path.as_deref(),
                    chunks,
                    permissions: metadata.permissions(),
                });
            }
        }
    }

    Ok(prepared)
}
