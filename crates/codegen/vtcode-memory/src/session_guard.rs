//! Stable per-session locks coordinating open handles with retention.

use std::ffi::OsStr;
use std::fs::File;
use std::path::Path;

use crate::SessionStoreError;

fn open_guard(workspace: &Path, name: &OsStr) -> Result<File, SessionStoreError> {
    let path = workspace.join(".vtcode/session-guards").join(name);
    vtcode_commons::VtCodePaths::open_private_append_file(&path)
        .map_err(|error| SessionStoreError::io(path, std::io::Error::other(error)))
}

pub(crate) fn shared(workspace: &Path, name: &OsStr) -> Result<File, SessionStoreError> {
    let file = open_guard(workspace, name)?;
    file.lock_shared().map_err(|error| SessionStoreError::io(workspace, error))?;
    Ok(file)
}

pub(crate) fn shared_for_session_dir(session_dir: &Path) -> Result<File, SessionStoreError> {
    let root = session_dir
        .parent()
        .ok_or_else(|| SessionStoreError::io(session_dir, std::io::Error::other("missing sessions root")))?;
    let vtcode_dir = root
        .parent()
        .filter(|_| root.file_name() == Some(OsStr::new("sessions")))
        .filter(|parent| parent.file_name() == Some(OsStr::new(".vtcode")))
        .ok_or_else(|| SessionStoreError::io(session_dir, std::io::Error::other("invalid session directory")))?;
    let workspace = vtcode_dir
        .parent()
        .ok_or_else(|| SessionStoreError::io(session_dir, std::io::Error::other("missing workspace")))?;
    let name = session_dir
        .file_name()
        .ok_or_else(|| SessionStoreError::io(session_dir, std::io::Error::other("missing session name")))?;
    shared(workspace, name)
}

pub(crate) fn try_exclusive(workspace: &Path, name: &OsStr) -> Result<Option<File>, SessionStoreError> {
    let file = open_guard(workspace, name)?;
    match file.try_lock().map_err(std::io::Error::from) {
        Ok(()) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(SessionStoreError::io(workspace, error)),
    }
}
