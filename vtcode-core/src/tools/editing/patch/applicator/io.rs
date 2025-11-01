use std::fs::Permissions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};

use super::PatchError;

pub(super) struct AtomicWriter {
    path: PathBuf,
    temp_path: PathBuf,
    writer: BufWriter<fs::File>,
    permissions: Option<Permissions>,
}

impl AtomicWriter {
    pub(super) async fn create(
        path: &Path,
        permissions: Option<Permissions>,
    ) -> Result<Self, PatchError> {
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
        let file = fs::File::create(&temp_path)
            .await
            .map_err(|err| PatchError::Io {
                action: "create",
                path: temp_path.clone(),
                source: err,
            })?;

        Ok(Self {
            path: path.to_path_buf(),
            temp_path,
            writer: BufWriter::new(file),
            permissions,
        })
    }

    pub(super) async fn write_all(&mut self, bytes: &[u8]) -> Result<(), PatchError> {
        self.writer
            .write_all(bytes)
            .await
            .map_err(|err| PatchError::Io {
                action: "write",
                path: self.temp_path.clone(),
                source: err,
            })
    }

    pub(super) async fn commit(mut self) -> Result<(), PatchError> {
        self.writer.flush().await.map_err(|err| PatchError::Io {
            action: "flush",
            path: self.temp_path.clone(),
            source: err,
        })?;
        drop(self.writer);

        if let Some(permissions) = self.permissions {
            fs::set_permissions(&self.temp_path, permissions)
                .await
                .map_err(|err| PatchError::Io {
                    action: "set permissions",
                    path: self.temp_path.clone(),
                    source: err,
                })?;
        }

        match fs::rename(&self.temp_path, &self.path).await {
            Ok(()) => Ok(()),
            Err(err) => {
                let _ = fs::remove_file(&self.temp_path).await;
                Err(PatchError::Io {
                    action: "rename",
                    path: self.path.clone(),
                    source: err,
                })
            }
        }
    }

    pub(super) async fn rollback(self) -> Result<(), PatchError> {
        drop(self.writer);
        match fs::remove_file(&self.temp_path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(PatchError::Io {
                action: "delete",
                path: self.temp_path,
                source: err,
            }),
        }
    }
}

#[derive(Clone)]
pub(super) struct BackupEntry {
    path: PathBuf,
    kind: BackupKind,
}

#[derive(Clone, Copy)]
enum BackupKind {
    File,
    Directory,
}

impl BackupEntry {
    async fn create(target: &Path, kind: BackupKind) -> Result<Self, PatchError> {
        let backup_path = temporary_path(target)?;
        fs::rename(target, &backup_path)
            .await
            .map_err(|err| PatchError::Io {
                action: "rename",
                path: target.to_path_buf(),
                source: err,
            })?;
        Ok(Self {
            path: backup_path,
            kind,
        })
    }

    pub(super) async fn remove(self) -> Result<(), PatchError> {
        match self.kind {
            BackupKind::File => match fs::remove_file(&self.path).await {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
                Err(err) => Err(PatchError::Io {
                    action: "delete",
                    path: self.path,
                    source: err,
                }),
            },
            BackupKind::Directory => match fs::remove_dir_all(&self.path).await {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
                Err(err) => Err(PatchError::Io {
                    action: "delete",
                    path: self.path,
                    source: err,
                }),
            },
        }
    }

    pub(super) async fn restore(&self, destination: &Path) -> Result<(), PatchError> {
        match fs::rename(&self.path, destination).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                remove_existing_entry(destination).await?;
                fs::rename(&self.path, destination)
                    .await
                    .map_err(|rename_err| PatchError::Io {
                        action: "rename",
                        path: destination.to_path_buf(),
                        source: rename_err,
                    })
            }
            Err(err) => Err(PatchError::Io {
                action: "restore",
                path: destination.to_path_buf(),
                source: err,
            }),
        }
    }
}

pub(super) async fn remove_existing_entry(path: &Path) -> Result<(), PatchError> {
    match fs::metadata(path).await {
        Ok(metadata) => {
            if metadata.is_dir() {
                fs::remove_dir_all(path)
                    .await
                    .map_err(|err| PatchError::Io {
                        action: "delete",
                        path: path.to_path_buf(),
                        source: err,
                    })?
            } else {
                fs::remove_file(path).await.map_err(|err| PatchError::Io {
                    action: "delete",
                    path: path.to_path_buf(),
                    source: err,
                })?
            }
            Ok(())
        }
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(PatchError::Io {
            action: "inspect",
            path: path.to_path_buf(),
            source: err,
        }),
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

pub(super) async fn snapshot_for(path: &Path) -> Result<Option<BackupEntry>, PatchError> {
    match fs::metadata(path).await {
        Ok(metadata) => {
            let kind = if metadata.is_dir() {
                BackupKind::Directory
            } else {
                BackupKind::File
            };
            let entry = BackupEntry::create(path, kind).await?;
            Ok(Some(entry))
        }
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(PatchError::Io {
            action: "inspect",
            path: path.to_path_buf(),
            source: err,
        }),
    }
}
