//! Confined workspace mutations: rename (and later delete) helpers.
//!
//! Split from the workspace file module to keep reads focused. Every helper
//! resolves its target at call time through the same confinement boundary as
//! the read path, so symlink escapes fail closed and callers can never address
//! paths outside the workspace. These functions perform filesystem work and
//! must run on a background executor, never in render.

use std::fs;
use std::path::{Path, PathBuf};

use super::resolve_attachment_path;

/// Validate a same-directory replacement name. The returned name is safe to
/// join onto the entry's confined parent: it holds no separators, so the join
/// cannot escape the directory. Shared predicate: the rename dialog and
/// [`rename_entry`] both enforce it, so UI and filesystem cannot drift.
pub(crate) fn validated_file_name(raw: &str) -> Result<String, String> {
    if raw.is_empty() || raw.chars().all(char::is_whitespace) {
        return Err("Choose a name that is not blank.".to_string());
    }
    if raw == "." || raw == ".." {
        return Err("“.” and “..” are reserved names.".to_string());
    }
    if raw.contains('/') {
        return Err("Names cannot contain “/”. Rename stays in the same folder.".to_string());
    }
    if raw.chars().any(|c| c.is_control()) {
        return Err("Names cannot contain control characters.".to_string());
    }
    Ok(raw.to_string())
}

/// Rename one workspace entry within its own directory, returning the new
/// workspace-relative path. Files and directories alike; renaming to the
/// current name is a no-op success. Fails closed on missing sources,
/// collisions, and anything outside the workspace — a failed rename leaves
/// the filesystem untouched.
pub(crate) fn rename_entry(root: &Path, rel_path: &str, new_name: &str) -> Result<String, String> {
    let name = validated_file_name(new_name)?;
    let root = vtcode_commons::paths::canonicalize(root).map_err(|error| error.to_string())?;
    let source = resolve_attachment_path(&root, Path::new(rel_path)).map_err(|_error| missing_entry())?;
    if fs::symlink_metadata(&source).is_err() {
        return Err(missing_entry());
    }
    let Some(parent) = source.parent() else {
        return Err(rust_i18n::t!("app.files.no_parent").to_string());
    };
    let destination = parent.join(&name);
    // Unreachable after validation (no separators), but fail closed rather
    // than trust the join.
    if destination.strip_prefix(&root).is_err() {
        return Err(missing_entry());
    }
    if destination == source {
        return Ok(rel_path.to_string());
    }
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(format!("“{name}” already exists in this folder. Choose a different name."));
    }
    fs::rename(&source, &destination).map_err(|error| error.to_string())?;
    Ok(match rel_path.rsplit_once('/') {
        Some((prefix, _)) => format!("{prefix}/{name}"),
        None => name,
    })
}

/// Move one workspace entry (file or directory) to the Trash, following
/// the Finder convention instead of deleting permanently. Fails closed on
/// missing sources and anything outside the workspace.
pub(crate) fn trash_entry(root: &Path, rel_path: &str) -> Result<(), String> {
    move_to_trash(&confined_target(root, rel_path)?)
}

/// Resolve `rel_path` inside `root`, refusing anything that escapes the
/// workspace or no longer exists. Shared so the system trash path and the
/// test sink apply the identical confinement boundary.
fn confined_target(root: &Path, rel_path: &str) -> Result<PathBuf, String> {
    let root = vtcode_commons::paths::canonicalize(root).map_err(|error| error.to_string())?;
    let target = resolve_attachment_path(&root, Path::new(rel_path)).map_err(|_error| missing_entry())?;
    if fs::symlink_metadata(&target).is_err() {
        return Err(missing_entry());
    }
    Ok(target)
}

/// Test-only sibling of [`trash_entry`] that moves the entry into
/// `trash_dir` instead of the user's real `~/.Trash`. Tests exercise the
/// confinement and "left the tree" behavior without leaving litter behind
/// on the developer's machine.
#[cfg(test)]
pub(crate) fn trash_entry_into(root: &Path, rel_path: &str, trash_dir: &Path) -> Result<(), String> {
    move_into(&confined_target(root, rel_path)?, trash_dir)
}

/// Move `target` into `trash_dir`, using Finder's `name 2.ext` collision
/// convention so a repeat run never overwrites an earlier entry.
#[cfg(test)]
fn move_into(target: &Path, trash_dir: &Path) -> Result<(), String> {
    let name = target.file_name().ok_or_else(missing_entry)?.to_string_lossy().into_owned();
    fs::create_dir_all(trash_dir).map_err(|error| error.to_string())?;
    let mut destination = trash_dir.join(&name);
    let mut sequence = 2;
    while fs::symlink_metadata(&destination).is_ok() {
        destination = trash_dir.join(format!("{name} {sequence}"));
        sequence += 1;
    }
    fs::rename(target, &destination).map_err(|error| error.to_string())
}

/// Trash one confined path through `NSFileManager`, the same API Finder
/// uses. Direct Foundation call instead of the `trash` crate: this binary is
/// macOS-only and already links `objc2-foundation`, so the crate would buy
/// one call at the price of eleven transitive dependencies. All bindings
/// are safe generated wrappers — no `unsafe` needed.
fn move_to_trash(path: &Path) -> Result<(), String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    // Non-UTF-8 paths fail closed: the tree only lists UTF-8 names, so this
    // is unreachable from the UI.
    let utf8 = path.to_str().ok_or_else(missing_entry)?;
    let url = NSURL::fileURLWithPath_relativeToURL(&NSString::from_str(utf8), None);
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|error| error.localizedDescription().to_string())
}

fn missing_entry() -> String {
    rust_i18n::t!("app.files.missing").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("temp dir")
    }

    #[test]
    fn valid_names_pass_through_exactly() {
        for name in [
            "notes.md",
            "with spaces.txt",
            "ünicode-名前.rs",
            ".hidden",
            "trailing. ",
            "back\\slash.txt",
            "a",
        ] {
            assert_eq!(validated_file_name(name).unwrap(), name, "{name} must survive");
        }
    }

    #[test]
    fn invalid_names_are_rejected() {
        for name in ["", "   ", ".", "..", "a/b", "/abs", "a\0b", "tab\there", "line\nbreak"] {
            assert!(validated_file_name(name).is_err(), "{name:?} must be rejected");
        }
    }

    #[test]
    fn rename_round_trips_files_and_directories() {
        let workspace = temp_workspace();
        fs::write(workspace.path().join("old.md"), "# Notes").unwrap();
        fs::create_dir(workspace.path().join("old-dir")).unwrap();
        fs::write(workspace.path().join("old-dir").join("inner.txt"), "inner").unwrap();

        assert_eq!(rename_entry(workspace.path(), "old.md", "new.md").unwrap(), "new.md");
        assert!(!workspace.path().join("old.md").exists());
        assert_eq!(fs::read_to_string(workspace.path().join("new.md")).unwrap(), "# Notes");

        assert_eq!(rename_entry(workspace.path(), "old-dir", "new-dir").unwrap(), "new-dir");
        assert!(!workspace.path().join("old-dir").exists());
        assert!(workspace.path().join("new-dir").join("inner.txt").exists());
    }

    #[test]
    fn rename_keeps_nested_paths_in_their_directory() {
        let workspace = temp_workspace();
        fs::create_dir_all(workspace.path().join("src/nested")).unwrap();
        fs::write(workspace.path().join("src/nested/mod.rs"), "code").unwrap();

        assert_eq!(rename_entry(workspace.path(), "src/nested/mod.rs", "lib.rs").unwrap(), "src/nested/lib.rs");
        assert!(workspace.path().join("src/nested/lib.rs").exists());
        assert!(!workspace.path().join("src/nested/mod.rs").exists());
    }

    #[test]
    fn rename_to_the_same_name_is_a_no_op_success() {
        let workspace = temp_workspace();
        fs::write(workspace.path().join("same.md"), "data").unwrap();
        assert_eq!(rename_entry(workspace.path(), "same.md", "same.md").unwrap(), "same.md");
        assert_eq!(fs::read_to_string(workspace.path().join("same.md")).unwrap(), "data");
    }

    #[test]
    fn rename_fails_closed_and_leaves_the_filesystem_untouched() {
        use std::os::unix::fs::symlink;
        let workspace = temp_workspace();
        let outside = temp_workspace();
        fs::write(workspace.path().join("keep.md"), "keep").unwrap();
        fs::write(workspace.path().join("taken.md"), "taken").unwrap();
        fs::write(outside.path().join("outside.rs"), "private").unwrap();
        symlink(outside.path(), workspace.path().join("escape")).unwrap();

        // Collision, missing source, traversal, symlink escape, invalid name.
        assert!(rename_entry(workspace.path(), "keep.md", "taken.md").is_err());
        assert!(rename_entry(workspace.path(), "missing.md", "new.md").is_err());
        assert!(rename_entry(workspace.path(), "../outside.rs", "new.rs").is_err());
        assert!(rename_entry(workspace.path(), "escape/outside.rs", "new.rs").is_err());
        assert!(rename_entry(workspace.path(), "keep.md", "sub/evil.md").is_err());

        // Nothing moved, nothing created.
        assert_eq!(fs::read_to_string(workspace.path().join("keep.md")).unwrap(), "keep");
        assert_eq!(fs::read_to_string(workspace.path().join("taken.md")).unwrap(), "taken");
        assert!(!workspace.path().join("new.md").exists());
        assert!(!workspace.path().join("new.rs").exists());
        assert!(!workspace.path().join("sub").exists());
        assert_eq!(fs::read_to_string(outside.path().join("outside.rs")).unwrap(), "private");
    }

    #[test]
    fn trash_moves_workspace_entries_out_of_the_tree() {
        // A sink directory stands in for `~/.Trash`: the real
        // `trashItemAtURL` call would deposit a permanent, user-visible
        // entry on the developer's machine on every run, and the assertion
        // below (entry left the tree, entry arrived intact) is what the
        // behavior actually guarantees. `trash_hits_the_system_trash` proves
        // the real binding separately.
        let workspace = temp_workspace();
        let sink = temp_workspace();
        fs::write(workspace.path().join("doomed.md"), "doomed").unwrap();
        fs::create_dir(workspace.path().join("doomed-dir")).unwrap();

        trash_entry_into(workspace.path(), "doomed.md", sink.path()).unwrap();
        assert!(!workspace.path().join("doomed.md").exists());
        assert_eq!(fs::read_to_string(sink.path().join("doomed.md")).unwrap(), "doomed");

        trash_entry_into(workspace.path(), "doomed-dir", sink.path()).unwrap();
        assert!(!workspace.path().join("doomed-dir").exists());
        assert!(sink.path().join("doomed-dir").is_dir());
    }

    #[test]
    fn trashing_into_the_same_sink_twice_keeps_both_entries() {
        // Collision convention: Finder never overwrites, so a second run
        // must land beside the first instead of destroying it.
        let workspace = temp_workspace();
        let sink = temp_workspace();
        fs::write(workspace.path().join("doomed.md"), "first").unwrap();
        trash_entry_into(workspace.path(), "doomed.md", sink.path()).unwrap();
        fs::write(workspace.path().join("doomed.md"), "second").unwrap();
        trash_entry_into(workspace.path(), "doomed.md", sink.path()).unwrap();

        assert_eq!(fs::read_to_string(sink.path().join("doomed.md")).unwrap(), "first");
        assert_eq!(fs::read_to_string(sink.path().join("doomed.md 2")).unwrap(), "second");
    }

    #[test]
    fn trash_fails_closed_without_touching_the_trash() {
        use std::os::unix::fs::symlink;
        let workspace = temp_workspace();
        let outside = temp_workspace();
        fs::write(workspace.path().join("keep.md"), "keep").unwrap();
        fs::write(outside.path().join("outside.rs"), "private").unwrap();
        symlink(outside.path(), workspace.path().join("escape")).unwrap();

        assert!(trash_entry(workspace.path(), "missing.md").is_err());
        assert!(trash_entry(workspace.path(), "../outside.rs").is_err());
        assert!(trash_entry(workspace.path(), "escape/outside.rs").is_err());

        assert_eq!(fs::read_to_string(workspace.path().join("keep.md")).unwrap(), "keep");
        assert_eq!(fs::read_to_string(outside.path().join("outside.rs")).unwrap(), "private");
    }

    #[test]
    #[ignore = "writes a real ~/.Trash entry; run with `--run-ignored` when touching the NSFileManager binding"]
    fn trash_hits_the_system_trash() {
        // Opt-in only: this is the one test that proves the real
        // `trashItemAtURL_resultingItemURL_error` binding, at the cost of a
        // permanent, user-visible Trash entry. Everything else uses the sink.
        let workspace = temp_workspace();
        let name = format!("vtcode-trash-probe-{}", std::process::id());
        fs::write(workspace.path().join(&name), "probe").unwrap();

        trash_entry(workspace.path(), &name).unwrap();
        assert!(!workspace.path().join(&name).exists(), "the entry must leave the workspace");
    }
}
