//! Cheap workspace environment-delta observability.
//!
//! Long-horizon agents operate in a *changing environment*: files are written,
//! processes mutate state, git trees move. The harness needs a fast way to
//! detect that the environment drifted between turns so it can re-ground
//! assumptions instead of trusting a stale picture.
//!
//! [`WorkspaceSnapshot`] captures a lightweight fingerprint of every tracked
//! file (size + mtime + a short content hash of the head), and [`diff`]
//! computes added/changed/removed paths. Snapshots are cheap enough to take at
//! each turn boundary and to persist as a derived view.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Directories never traversed when capturing a snapshot.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".svn",
    "target",
    "node_modules",
    "dist",
    "build",
    ".vtcode",
    ".next",
    "vendor",
];

/// A compact fingerprint of a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStat {
    /// Size in bytes.
    pub size: u64,
    /// Modification time in nanoseconds since the Unix epoch.
    pub mtime_ns: i64,
    /// FNV-1a hash of the file's first 4096 bytes.
    pub head_hash: u64,
}

/// Number of leading bytes sampled for the content fingerprint.
const HASH_SAMPLE_BYTES: usize = 4096;

/// A point-in-time fingerprint of a workspace's tracked files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    /// `relative_path -> stat`, sorted for stable diffs.
    pub files: BTreeMap<String, FileStat>,
    /// RFC3339 capture timestamp.
    pub captured_at: String,
}

/// The difference between two workspace snapshots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotDelta {
    /// Paths present in `new` but not `old`.
    pub added: Vec<String>,
    /// Paths present in both but with a different fingerprint.
    pub changed: Vec<String>,
    /// Paths present in `old` but not `new`.
    pub removed: Vec<String>,
}

/// Capture a snapshot of `workspace`, skipping VCS/ build/ cache directories.
///
/// Files larger than `max_file_bytes` are fingerprinted by size + mtime only
/// (the head hash is set to 0) to keep capture O(files) rather than O(bytes).
pub fn capture(workspace: &Path, max_file_bytes: u64) -> io::Result<WorkspaceSnapshot> {
    let mut files = BTreeMap::new();
    collect(workspace, workspace, max_file_bytes, &mut files)?;
    Ok(WorkspaceSnapshot { files, captured_at: now_rfc3339() })
}

fn collect(root: &Path, dir: &Path, max_file_bytes: u64, out: &mut BTreeMap<String, FileStat>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && SKIP_DIRS.contains(&name)
            {
                continue;
            }
            collect(root, &path, max_file_bytes, out)?;
        } else if ft.is_file() {
            let rel = match path.strip_prefix(root).ok().and_then(|p| p.to_str()) {
                Some(r) => r.to_string(),
                None => continue,
            };
            match entry.metadata() {
                Ok(meta) => {
                    let size = meta.len();
                    let mtime_ns = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos() as i64)
                        .unwrap_or(0);
                    let head_hash = if size <= max_file_bytes { hash_head(&path) } else { 0 };
                    out.insert(rel, FileStat { size, mtime_ns, head_hash });
                }
                Err(_) => continue,
            }
        }
    }
    Ok(())
}

/// FNV-1a 64-bit hash of the first [`HASH_SAMPLE_BYTES`] bytes of `path`.
fn hash_head(path: &Path) -> u64 {
    const SEED: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = SEED;
    let mut buf = [0u8; HASH_SAMPLE_BYTES];
    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::Read;
        if let Ok(n) = f.read(&mut buf) {
            for &b in &buf[..n] {
                hash ^= u64::from(b);
                hash = hash.wrapping_mul(PRIME);
            }
        }
    }
    hash
}

/// Compute the delta from `old` to `new`.
#[must_use]
pub fn diff(old: &WorkspaceSnapshot, new: &WorkspaceSnapshot) -> SnapshotDelta {
    let mut delta = SnapshotDelta::default();
    for (path, new_stat) in &new.files {
        match old.files.get(path) {
            None => delta.added.push(path.clone()),
            Some(old_stat) if old_stat != new_stat => delta.changed.push(path.clone()),
            Some(_) => {}
        }
    }
    for path in old.files.keys() {
        if !new.files.contains_key(path) {
            delta.removed.push(path.clone());
        }
    }
    delta
}

/// Whether the delta indicates meaningful environment drift.
#[must_use]
pub fn is_drift(delta: &SnapshotDelta) -> bool {
    !delta.added.is_empty() || !delta.changed.is_empty() || !delta.removed.is_empty()
}

/// Persist a snapshot as JSON (e.g. as a session derived view).
pub fn save_json(snapshot: &WorkspaceSnapshot, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(snapshot).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, bytes)
}

/// Load a previously persisted snapshot.
pub fn load_json(path: &Path) -> io::Result<WorkspaceSnapshot> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Resolve the on-disk path for a session's environment snapshot.
#[must_use]
pub fn snapshot_path(session_dir: &Path) -> PathBuf {
    session_dir.join("derived").join("workspace_snapshot.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_and_diff_detects_changes() {
        let tmp = std::env::temp_dir().join(format!("vtcode-snap-{}", std::process::id()));
        let ws = tmp.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("a.txt"), b"hello").unwrap();
        std::fs::write(ws.join("b.txt"), b"world").unwrap();

        let snap1 = capture(&ws, 1_000_000).unwrap();
        assert_eq!(snap1.files.len(), 2);
        assert!(snap1.files.contains_key("a.txt"));
        assert!(snap1.files.contains_key("b.txt"));

        // Mutate + add + remove.
        std::fs::write(ws.join("a.txt"), b"changed").unwrap();
        std::fs::write(ws.join("c.txt"), b"new").unwrap();
        std::fs::remove_file(ws.join("b.txt")).unwrap();

        let snap2 = capture(&ws, 1_000_000).unwrap();
        let delta = diff(&snap1, &snap2);
        assert_eq!(delta.added, vec!["c.txt".to_string()]);
        assert_eq!(delta.changed, vec!["a.txt".to_string()]);
        assert_eq!(delta.removed, vec!["b.txt".to_string()]);
        assert!(is_drift(&delta));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn json_round_trips() {
        let tmp = std::env::temp_dir().join(format!("vtcode-snap-json-{}", std::process::id()));
        let path = tmp.join("snap.json");
        let snap = WorkspaceSnapshot {
            files: {
                let mut m = BTreeMap::new();
                m.insert("x.rs".to_string(), FileStat { size: 3, mtime_ns: 42, head_hash: 7 });
                m
            },
            captured_at: "2026-01-01T00:00:00Z".to_string(),
        };
        save_json(&snap, &path).unwrap();
        let loaded = load_json(&path).unwrap();
        assert_eq!(loaded, snap);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
