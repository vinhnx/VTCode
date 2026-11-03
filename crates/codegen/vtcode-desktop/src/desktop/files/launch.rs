//! External application launches: editors and terminals.
//!
//! Split from the workspace file module to keep files focused. Every launch
//! resolves its target at launch time and runs a fixed argv, never a shell.

use std::fs;
use std::path::{Path, PathBuf};

use super::{ExternalEditor, OpenAction, resolve_attachment_path};

fn editor_command(editor: ExternalEditor, path: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/open");
    match editor {
        ExternalEditor::System => {
            let _command = command.arg("-t");
        }
        ExternalEditor::Finder => {
            let _command = command.arg("-R");
        }
        editor => match editor.bundle_id() {
            Some(id) => {
                let _command = command.args(["-b", id]);
            }
            // Only System and Finder lack identifiers, and both match above.
            // Fall back to the system default rather than launching nothing.
            None => {
                let _command = command.arg("-t");
            }
        },
    }
    let _command = command.arg(path);
    command
}

/// Resolve the launch target: the file itself, or its containing directory
/// for terminal emulators. Pure filesystem work, safe to unit test.
fn launch_path(root: &Path, rel_path: &str, editor: ExternalEditor) -> Result<PathBuf, String> {
    let path = resolve_attachment_path(root, Path::new(rel_path))
        .map_err(|_error| rust_i18n::t!("app.files.missing").to_string())?;
    if !fs::metadata(&path).map(|metadata| metadata.is_file()).unwrap_or(false) {
        return Err(rust_i18n::t!("app.files.not_regular").to_string());
    }
    if editor.open_action() == OpenAction::OpenDirectory {
        return path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| rust_i18n::t!("app.files.no_parent").to_string());
    }
    Ok(path)
}

/// Resolve the file again at launch time, and never pass a shell command.
/// This function performs filesystem and process work and must run in the background.
pub(crate) fn open_in_editor(root: &Path, rel_path: &str, editor: ExternalEditor) -> Result<(), String> {
    let path = launch_path(root, rel_path, editor)?;
    run_open_command(editor_command(editor, &path))
}

/// Reveal one workspace entry (file or directory) in Finder: `open -R`
/// selects the item in its parent folder. Unlike [`open_in_editor`],
/// directories are accepted — revealing answers "where is this item"
/// for both kinds. Confined to the workspace like every other launch;
/// symlink escapes and missing entries fail closed. Performs filesystem
/// and process work and must run in the background.
pub(crate) fn reveal_in_finder(root: &Path, rel_path: &str) -> Result<(), String> {
    let path = resolve_attachment_path(root, Path::new(rel_path))
        .map_err(|_error| rust_i18n::t!("app.files.missing").to_string())?;
    if !fs::metadata(&path).is_ok() {
        return Err(rust_i18n::t!("app.files.missing").to_string());
    }
    run_open_command(editor_command(ExternalEditor::Finder, &path))
}

/// Open Terminal at a workspace root (no file anchor needed, e.g. for the
/// CLI handoff card). Same argv-boundary discipline as [`open_in_editor`]:
/// a fixed argv, never a shell command.
pub(crate) fn open_terminal_at(root: &Path) -> Result<(), String> {
    if !root.is_dir() {
        return Err(rust_i18n::t!("app.files.missing").to_string());
    }
    run_open_command(editor_command(ExternalEditor::Terminal, root))
}

fn run_open_command(mut command: std::process::Command) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| rust_i18n::t!("app.files.launch_failed", error = error.to_string()).to_string())?;
    interpret_open_output(&output)
}

fn interpret_open_output(output: &std::process::Output) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail: String = detail.chars().filter(|c| !c.is_control() || *c == ' ').take(300).collect();
        Err(if detail.trim().is_empty() {
            rust_i18n::t!("app.files.open_failed").to_string()
        } else {
            rust_i18n::t!("app.files.open_failed_detail", detail = detail.trim()).to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::ExternalEditor;
    use super::*;

    fn temp_workspace() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("temp dir")
    }

    #[test]
    fn editor_launch_uses_argument_boundaries_and_confines_paths() {
        use std::os::unix::fs::symlink;
        let workspace = temp_workspace();
        let outside = temp_workspace();
        let name = "-weird $(touch nope).rs";
        fs::write(workspace.path().join(name), "hello").unwrap();
        fs::write(outside.path().join("outside.rs"), "private").unwrap();
        symlink(outside.path(), workspace.path().join("escape")).unwrap();
        let path = vtcode_commons::paths::resolve_workspace_path(workspace.path(), Path::new(name)).unwrap();
        // Independent oracle: every variant's launch flags, stated separately
        // from the implementation.
        for editor in ExternalEditor::ALL {
            let expected: &[&str] = match editor {
                ExternalEditor::System => &["-t"],
                ExternalEditor::Finder => &["-R"],
                ExternalEditor::VsCode => &["-b", "com.microsoft.VSCode"],
                ExternalEditor::Sublime => &["-b", "com.sublimetext.4"],
                ExternalEditor::Xcode => &["-b", "com.apple.dt.Xcode"],
                ExternalEditor::AndroidStudio => &["-b", "com.google.android.studio"],
                ExternalEditor::Terminal => &["-b", "com.apple.Terminal"],
                ExternalEditor::ITerm2 => &["-b", "com.googlecode.iterm2"],
                ExternalEditor::Ghostty => &["-b", "com.mitchellh.ghostty"],
            };
            let command = editor_command(editor, &path);
            let args = command.get_args().collect::<Vec<_>>();
            assert_eq!(args.len(), expected.len() + 1, "{editor:?} flags");
            for (actual, expected) in args.iter().zip(expected) {
                assert_eq!(*actual, *expected);
            }
            assert_eq!(args.last().unwrap(), &path.as_os_str());
        }
        assert!(open_in_editor(workspace.path(), "escape/outside.rs", ExternalEditor::System).is_err());
        assert!(open_in_editor(workspace.path(), "missing", ExternalEditor::VsCode).is_err());
        assert!(open_in_editor(workspace.path(), ".", ExternalEditor::System).is_err());
        assert!(!workspace.path().join("nope").exists());
    }

    #[test]
    fn terminal_at_workspace_root_rejects_missing_directories() {
        // Success launches a real Terminal, so only the fail-closed guard is
        // pinned here; the launch itself is covered by view interception tests.
        let workspace = temp_workspace();
        assert!(open_terminal_at(&workspace.path().join("missing")).is_err());
    }

    #[test]
    fn launch_paths_resolve_files_and_terminal_directories_without_shelling_out() {
        let workspace = temp_workspace();
        let nested = workspace.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("deep.rs"), "code").unwrap();
        fs::write(workspace.path().join("top.rs"), "code").unwrap();
        let root = vtcode_commons::paths::canonicalize(workspace.path()).unwrap();

        for editor in [ExternalEditor::VsCode, ExternalEditor::Finder, ExternalEditor::System] {
            assert_eq!(
                launch_path(&root, "nested/deep.rs", editor).unwrap(),
                root.join("nested/deep.rs"),
                "{editor:?} targets the file"
            );
        }
        for editor in [
            ExternalEditor::Terminal,
            ExternalEditor::ITerm2,
            ExternalEditor::Ghostty,
        ] {
            assert_eq!(
                launch_path(&root, "nested/deep.rs", editor).unwrap(),
                root.join("nested"),
                "{editor:?} targets the containing directory"
            );
        }
        assert_eq!(
            launch_path(&root, "top.rs", ExternalEditor::Terminal).unwrap(),
            root,
            "a root-level file opens the workspace root itself"
        );
        assert!(launch_path(&root, "missing.rs", ExternalEditor::Ghostty).is_err());
        assert!(launch_path(&root, "../escape.rs", ExternalEditor::Terminal).is_err());
        assert!(launch_path(&root, "nested", ExternalEditor::VsCode).is_err(), "directories are rejected");
    }

    #[test]
    fn reveal_rejects_missing_and_escaping_paths_without_launching() {
        use std::os::unix::fs::symlink;
        // The success path spawns Finder, so only the fail-closed guards are
        // pinned here; the launch itself is covered by view interception tests.
        let workspace = temp_workspace();
        let outside = temp_workspace();
        fs::write(outside.path().join("outside.rs"), "private").unwrap();
        symlink(outside.path(), workspace.path().join("escape")).unwrap();
        assert!(reveal_in_finder(workspace.path(), "missing.rs").is_err());
        assert!(reveal_in_finder(workspace.path(), "../escape.rs").is_err());
        assert!(reveal_in_finder(workspace.path(), "escape/outside.rs").is_err());
    }
}
