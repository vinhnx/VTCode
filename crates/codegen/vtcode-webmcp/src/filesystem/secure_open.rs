use std::io::Read;
#[cfg(all(unix, not(any(target_os = "redox", target_os = "solaris"))))]
use std::io::{Seek, SeekFrom, Write};
#[cfg(all(unix, not(any(target_os = "redox", target_os = "solaris"))))]
use std::path::Component;
use std::path::Path;

use crate::error::{Result, WebmcpError};
use crate::runtime::FileSnapshot;

use super::{FilesystemLimits, digest_text, reject_hard_links, reject_sensitive_relative_path, validate_relative_path};

#[cfg(all(unix, not(any(target_os = "redox", target_os = "solaris"))))]
pub(super) fn open_root_directory(path: &Path) -> std::io::Result<std::fs::File> {
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::Mode;
    use std::os::unix::fs::OpenOptionsExt;

    let mut current = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            if matches!(component, Component::RootDir | Component::CurDir) {
                continue;
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "canonical workspace root contains an unsupported path component",
            ));
        };
        let directory = openat(
            &current,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        current = std::fs::File::from(directory);
    }
    Ok(current)
}

#[cfg(not(all(unix, not(any(target_os = "redox", target_os = "solaris")))))]
pub(super) fn open_root_directory(_path: &Path) -> Result<std::fs::File> {
    Err(WebmcpError::Unsupported(
        "WebMCP filesystem access requires directory-handle file operations on this platform".to_string(),
    ))
}

pub(super) fn read_snapshot_blocking(
    root: &Path,
    root_dir: &std::fs::File,
    path: &str,
    limits: FilesystemLimits,
) -> Result<FileSnapshot> {
    let relative = validate_relative_path(path)?;
    reject_sensitive_relative_path(&relative, path)?;
    let file = open_workspace_file(root, root_dir, &relative, false)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(WebmcpError::PathRejected(path.to_string()));
    }
    reject_hard_links(&metadata, path)?;
    let max_file_bytes = u64::try_from(limits.max_file_bytes).unwrap_or(u64::MAX);
    if metadata.len() > max_file_bytes {
        return Err(WebmcpError::LimitExceeded);
    }
    let mut file = file;
    let content = read_bounded_content(&mut file, limits.max_file_bytes)?;
    let digest = digest_text(&content);
    Ok(FileSnapshot { path: path.to_string(), content, digest })
}

fn read_bounded_content<R>(reader: &mut R, max_file_bytes: usize) -> Result<String>
where
    R: Read,
{
    let max_file_bytes_u64 = u64::try_from(max_file_bytes).unwrap_or(u64::MAX);
    let mut bytes = Vec::with_capacity(max_file_bytes.min(64 * 1024));
    let _bytes_read = reader.take(max_file_bytes_u64.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() > max_file_bytes {
        return Err(WebmcpError::LimitExceeded);
    }
    String::from_utf8(bytes).map_err(|_error| WebmcpError::Adapter("file is not valid UTF-8".to_string()))
}

#[cfg(all(unix, not(any(target_os = "redox", target_os = "solaris"))))]
pub(super) fn open_workspace_file(
    _root: &Path,
    root_dir: &std::fs::File,
    relative: &Path,
    write: bool,
) -> Result<std::fs::File> {
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::Mode;

    let (parent, name) = open_parent_directory(root_dir, relative)?;
    let access = if write { OFlag::O_RDWR } else { OFlag::O_RDONLY };
    let file = openat(&parent, name.as_os_str(), access | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW, Mode::empty())
        .map_err(|error| map_secure_open_error(relative, error))?;
    Ok(std::fs::File::from(file))
}

#[cfg(all(unix, not(any(target_os = "redox", target_os = "solaris"))))]
fn open_parent_directory(root_dir: &std::fs::File, relative: &Path) -> Result<(std::fs::File, std::ffi::OsString)> {
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::Mode;

    let mut components = relative.components();
    let Some(Component::Normal(name)) = components.next_back() else {
        return Err(WebmcpError::PathRejected(relative.display().to_string()));
    };
    let mut current = root_dir.try_clone()?;
    for component in components {
        let Component::Normal(name) = component else {
            return Err(WebmcpError::PathRejected(relative.display().to_string()));
        };
        let directory = openat(
            &current,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|error| map_secure_open_error(relative, error))?;
        current = std::fs::File::from(directory);
    }
    Ok((current, name.to_os_string()))
}

#[cfg(all(unix, not(any(target_os = "redox", target_os = "solaris"))))]
fn map_secure_open_error(relative: &Path, error: nix::errno::Errno) -> WebmcpError {
    if matches!(error, nix::errno::Errno::ELOOP | nix::errno::Errno::ENOTDIR) {
        WebmcpError::PathRejected(format!("symlink path is not allowed: {}", relative.display()))
    } else {
        WebmcpError::Io(error.into())
    }
}

#[cfg(not(all(unix, not(any(target_os = "redox", target_os = "solaris")))))]
pub(super) fn open_workspace_file(
    _root: &Path,
    _root_dir: &std::fs::File,
    _relative: &Path,
    _write: bool,
) -> Result<std::fs::File> {
    Err(WebmcpError::Unsupported(
        "WebMCP filesystem access requires directory-handle file operations on this platform".to_string(),
    ))
}

pub(super) fn replace_snapshot_if_current_blocking(
    root: &Path,
    root_dir: &std::fs::File,
    update: &FileSnapshot,
    expected: &FileSnapshot,
) -> Result<()> {
    let relative = validate_relative_path(&expected.path)?;
    reject_sensitive_relative_path(&relative, &expected.path)?;
    if update.path != expected.path {
        return Err(WebmcpError::Adapter("compare-and-replace paths do not match".to_string()));
    }
    if digest_text(&update.content) != update.digest {
        return Err(WebmcpError::Adapter("snapshot digest does not match its content".to_string()));
    }
    let file = open_workspace_file(root, root_dir, &relative, true)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(WebmcpError::PathRejected(expected.path.clone()));
    }
    reject_hard_links(&metadata, &expected.path)?;
    replace_open_file(file, update, expected)
}

#[cfg(all(unix, not(any(target_os = "redox", target_os = "solaris"))))]
fn replace_open_file(file: std::fs::File, update: &FileSnapshot, expected: &FileSnapshot) -> Result<()> {
    use nix::fcntl::{Flock, FlockArg};

    let mut file =
        Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, error)| WebmcpError::Io(error.into()))?;
    let metadata_before = file.metadata()?;
    if !metadata_before.is_file() {
        return Err(WebmcpError::PathRejected(expected.path.clone()));
    }
    reject_hard_links(&metadata_before, &expected.path)?;
    let expected_size = u64::try_from(expected.content.len()).unwrap_or(u64::MAX);
    if metadata_before.len() > expected_size {
        return Err(WebmcpError::Conflict {
            path: expected.path.clone(),
            expected: expected.digest.clone(),
            actual: format!("size:{}", metadata_before.len()),
        });
    }
    let _ = file.seek(SeekFrom::Start(0))?;
    let current = match read_bounded_content(&mut *file, expected.content.len()) {
        Ok(current) => current,
        Err(WebmcpError::LimitExceeded) => {
            return Err(WebmcpError::Conflict {
                path: expected.path.clone(),
                expected: expected.digest.clone(),
                actual: format!("size:>{expected_size}"),
            });
        }
        Err(error) => return Err(error),
    };
    let metadata_after_read = file.metadata()?;
    if metadata_after_read.len() > expected_size || metadata_after_read.len() != metadata_before.len() {
        return Err(WebmcpError::Conflict {
            path: expected.path.clone(),
            expected: expected.digest.clone(),
            actual: format!("size:{}", metadata_after_read.len()),
        });
    }
    if metadata_after_read.modified().ok() != metadata_before.modified().ok() {
        return Err(WebmcpError::Conflict {
            path: expected.path.clone(),
            expected: expected.digest.clone(),
            actual: "metadata-changed".to_string(),
        });
    }
    let actual = digest_text(&current);
    if actual != expected.digest {
        return Err(WebmcpError::Conflict {
            path: expected.path.clone(),
            expected: expected.digest.clone(),
            actual,
        });
    }
    let _ = file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    file.write_all(update.content.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

#[cfg(not(all(unix, not(any(target_os = "redox", target_os = "solaris")))))]
fn replace_open_file(_file: std::fs::File, _update: &FileSnapshot, _expected: &FileSnapshot) -> Result<()> {
    Err(WebmcpError::Unsupported(
        "WebMCP compare-and-replace requires a platform with directory-handle file operations".to_string(),
    ))
}
