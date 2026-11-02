//! Contained atomic file writes for index and markdown storage.
//!
//! Workspace checkouts are untrusted input: a planted symlink must neither
//! redirect a write outside its directory nor turn a predictable temporary
//! name into a truncation target. Snapshot and record writes resolve their
//! directory with [`canonical_storage_dir`], then publish bytes with
//! [`write_atomic`], which lands them in a uniquely named exclusive
//! temporary file and renames it into place.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use vtcode_commons::{canonicalize, validate_no_escaping_symlink_ancestors};

/// Resolve `dir` to the canonical directory all writes must stay inside.
///
/// The directory is created when absent. Every existing component is
/// checked for escaping symlinks first, so a planted link anywhere along
/// the path fails closed instead of redirecting the whole store elsewhere;
/// canonicalization then binds every later join to the true location.
pub(crate) fn canonical_storage_dir(dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dir).with_context(|| format!("could not create storage directory {}", dir.display()))?;
    resolve_canonical_storage_dir(dir)
}

/// Like [`canonical_storage_dir`] but never creates anything: a missing
/// store fails instead, keeping read paths side-effect free.
pub(crate) fn canonical_existing_storage_dir(dir: &Path) -> Result<PathBuf> {
    resolve_canonical_storage_dir(dir)
}

fn resolve_canonical_storage_dir(dir: &Path) -> Result<PathBuf> {
    validate_no_escaping_symlink_ancestors(dir, false)
        .with_context(|| format!("could not validate storage directory {}", dir.display()))?;
    let metadata =
        fs::symlink_metadata(dir).with_context(|| format!("could not inspect storage directory {}", dir.display()))?;
    if metadata.file_type().is_symlink() {
        bail!("refusing to use symlinked directory {}", dir.display());
    }
    if !metadata.is_dir() {
        bail!("storage path {} is not a directory", dir.display());
    }
    canonicalize(dir).with_context(|| format!("could not resolve storage directory {}", dir.display()))
}

/// Check that `key` is a single file name with no separators, parent
/// components, or NUL bytes.
///
/// Shared by every record store so key validation cannot drift between
/// callers: separators, `..`, absolute paths, and empty keys all fail
/// closed, keeping record operations inside the canonical store directory.
pub(crate) fn validate_record_key(key: &str) -> Result<()> {
    if key.contains('\0') {
        bail!("invalid record key: NUL byte");
    }
    let mut components = Path::new(key).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => bail!("invalid record key {key:?}: expected a single file name"),
    }
}

/// Resolve `key` to `dir.join("{key}.{extension}")` inside the canonical
/// store directory, validating the key first.
///
/// This is the only path constructor record stores may use for
/// key-derived file names.
pub(crate) fn contained_record_path(dir: &Path, key: &str, extension: &str) -> Result<PathBuf> {
    validate_record_key(key)?;
    Ok(canonical_storage_dir(dir)?.join(format!("{key}.{extension}")))
}

/// Like [`contained_record_path`] but never creates the store: read and
/// delete paths resolve through this so they stay side-effect free.
pub(crate) fn contained_existing_record_path(dir: &Path, key: &str, extension: &str) -> Result<PathBuf> {
    validate_record_key(key)?;
    Ok(canonical_existing_storage_dir(dir)?.join(format!("{key}.{extension}")))
}

/// Reject symlink (or non-regular) destinations before a write or lock.
///
/// Rename-based publishing never follows a leaf symlink, but failing closed
/// here keeps the store's contents exactly what this process wrote.
///
/// Residual limitation: like the rest of this module, the check-then-act
/// sequence assumes no actor swaps path components concurrently with the
/// call. Planted at-rest symlinks — the realistic threat from an untrusted
/// checkout — are defeated; a live concurrent swapper racing these calls is
/// not.
pub(crate) fn reject_symlink_destination(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("refusing to replace symlinked file {}", path.display())
        }
        Ok(metadata) if !metadata.is_file() => bail!("{} is not a regular file", path.display()),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("could not inspect {}", path.display())),
    }
}

/// Atomically write `contents` to `dir.join(file_name)`.
///
/// `dir` must be a canonical store directory from [`canonical_storage_dir`]
/// or [`canonical_existing_storage_dir`]; `file_name` must be a validated
/// single-component name. The payload lands in a uniquely named exclusive
/// temporary file that is synced and renamed over the destination, so
/// concurrent readers observe the old or the new file but never a mix.
pub(crate) fn write_atomic(dir: &Path, file_name: &str, contents: &[u8]) -> Result<()> {
    let destination = dir.join(file_name);
    reject_symlink_destination(&destination)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    for attempt in 0..32u32 {
        let temporary = dir.join(format!(".{file_name}.{}-{timestamp}.{attempt}.tmp", std::process::id()));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).with_context(|| format!("could not create {}", temporary.display())),
        };
        if let Err(error) = file.write_all(contents).and_then(|()| file.sync_all()) {
            drop(file);
            drop(fs::remove_file(&temporary));
            return Err(error).with_context(|| format!("could not stage {}", destination.display()));
        }
        drop(file);
        if let Err(error) = fs::rename(&temporary, &destination) {
            drop(fs::remove_file(&temporary));
            return Err(error).with_context(|| format!("could not publish {}", destination.display()));
        }
        return Ok(());
    }
    bail!("could not allocate a unique temporary file in {}", dir.display())
}
