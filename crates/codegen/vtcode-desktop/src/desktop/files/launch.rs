//! External application launches: editors and terminals.
//!
//! Split from the workspace file module to keep files focused. Every launch
//! resolves its target at launch time and runs a fixed argv, never a shell.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// CLI flag the desktop handoff uses to continue a thread: a fork (not
/// `--resume`) because the desktop service holds the session's exclusive lock
/// while the thread is open. Shared with the display builder in
/// `view/settings_connections.rs` so the shown command and the auto-run
/// script can never disagree on the argv.
pub(crate) const CLI_FORK_SESSION_FLAG: &str = "--fork-session";

/// Whether `open -b <bundle> <script>` executes a `.command` handoff script
/// in this terminal. Only Terminal.app is verified end-to-end; iTerm2/Ghostty
/// `.command` handling is unconfirmed, so they stay on copy+open until a
/// manual run proves otherwise.
pub(crate) fn terminal_runs_handoff_script(terminal: ExternalEditor) -> bool {
    matches!(terminal, ExternalEditor::Terminal)
}

/// Build the `.command` script that continues `session_id` in a fresh
/// terminal: `cd` to the workspace, then `exec` the version-matched `vtcode`
/// binary. Every interpolated value is single-quoted, so hostile characters
/// (`$()`, backticks, `;`, newlines, unicode) stay literal. A trailing echo
/// keeps failures visible instead of opening an empty terminal. Returns
/// `None` when a path is not UTF-8: the caller falls back to copy+open
/// rather than quoting a lossy path.
pub(crate) fn build_handoff_script(vtcode: &Path, workspace: &Path, session_id: &str) -> Option<String> {
    let vtcode = vtcode.to_str()?;
    let workspace = workspace.to_str()?;
    Some(format!(
        "#!/bin/sh\ncd {} && exec {} {} {}\necho \"vtcode handoff failed with exit $?\"\nexit 1\n",
        shell_quote(workspace),
        shell_quote(vtcode),
        CLI_FORK_SESSION_FLAG,
        shell_quote(session_id),
    ))
}

/// Single-quote one shell word: `'` becomes `'\''` (close, escaped quote,
/// reopen). Newlines and `$()` stay literal inside single quotes.
fn shell_quote(word: &str) -> String {
    let mut quoted = String::with_capacity(word.len() + 2);
    quoted.push('\'');
    for (index, part) in word.split('\'').enumerate() {
        if index > 0 {
            quoted.push_str("'\\''");
        }
        quoted.push_str(part);
    }
    quoted.push('\'');
    quoted
}

/// Temp-file identities, shared by the writers and the stale sweep so the
/// sweep can never touch another file.
const HANDOFF_TEMP_PREFIX: &str = "vtcode-handoff-";
const PREVIEW_TEMP_PREFIX: &str = "vtcode-diagram-";
const PREVIEW_TEMP_EXTENSION: &str = "html";
/// Previews older than this are removed on the next preview open. A page
/// only needs to outlive the browser load; a day is generous.
const PREVIEW_TEMP_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
/// Upper bound per sweep so a pathological temp dir cannot stall a click.
const SWEEP_MAX_REMOVE: usize = 50;

/// Write a handoff script to an owner-only temp file, exclusively created so
/// a pre-existing symlink can never hijack the path. Names carry pid + a
/// per-process counter so concurrent handoffs never collide. No cleanup: the
/// OS purges temp, and deleting eagerly would race the terminal launch.
pub(crate) fn write_handoff_script(content: &str) -> Result<PathBuf, String> {
    write_owner_only_temp(HANDOFF_TEMP_PREFIX, "command", 0o700, content, |error| {
        rust_i18n::t!("app.files.launch_failed", error = error.to_string()).to_string()
    })
}

/// Write a diagram preview page to an owner-only temp file. Same
/// exclusive-create + pid/counter discipline as handoff scripts, without
/// the exec bit HTML does not need. No cleanup here: the OS purges temp,
/// deleting eagerly would race the browser launch, and
/// [`sweep_stale_previews`] reaps old pages on the next open.
pub(crate) fn write_preview_html(content: &str) -> Result<PathBuf, String> {
    write_owner_only_temp(PREVIEW_TEMP_PREFIX, PREVIEW_TEMP_EXTENSION, 0o600, content, |error| {
        rust_i18n::t!("app.markdown.diagram_open_failed_detail", detail = error.to_string()).to_string()
    })
}

/// Remove diagram previews older than [`PREVIEW_TEMP_TTL`], returning how
/// many were removed. Best-effort by design: every I/O error is ignored
/// so a hostile temp dir can never break opening a preview. Only files
/// written by [`write_preview_html`] (exact prefix + extension, regular
/// files only) are candidates, and unmeasurable ages are treated as fresh.
pub(crate) fn sweep_stale_previews() -> usize {
    sweep_previews_older_than(&std::env::temp_dir(), PREVIEW_TEMP_TTL)
}

fn sweep_previews_older_than(dir: &Path, ttl: std::time::Duration) -> usize {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return 0,
    };
    let mut removed = 0;
    for entry in entries.flatten().take(SWEEP_MAX_REMOVE * 4) {
        if removed >= SWEEP_MAX_REMOVE {
            break;
        }
        if !is_preview_temp_file(&entry) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| preview_is_stale(age, ttl));
        if stale && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn is_preview_temp_file(entry: &fs::DirEntry) -> bool {
    entry.file_type().map(|kind| kind.is_file()).unwrap_or(false)
        && entry.file_name().to_string_lossy().starts_with(PREVIEW_TEMP_PREFIX)
        && entry.path().extension().and_then(|ext| ext.to_str()) == Some(PREVIEW_TEMP_EXTENSION)
}

/// Staleness policy for preview temp files, shared by the sweep and its
/// boundary tests.
fn preview_is_stale(age: std::time::Duration, ttl: std::time::Duration) -> bool {
    age >= ttl
}

fn write_owner_only_temp(
    prefix: &str,
    extension: &str,
    mode: u32,
    content: &str,
    write_error: impl Fn(std::io::Error) -> String,
) -> Result<PathBuf, String> {
    let _ = mode;
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("{prefix}{}-{id}.{extension}", std::process::id()));
    let mut options = fs::OpenOptions::new();
    let _options = options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let _options = options.mode(mode);
    }
    let mut file = options.open(&path).map_err(&write_error)?;
    file.write_all(content.as_bytes()).map_err(write_error)?;
    Ok(path)
}

/// Open a handoff script in the terminal: fixed `open -b <bundle> <script>`
/// argv, never a shell command. Callers must check
/// [`terminal_runs_handoff_script`] first.
pub(crate) fn open_script_in_terminal(script: &Path, terminal: ExternalEditor) -> Result<(), String> {
    debug_assert!(terminal_runs_handoff_script(terminal), "handoff scripts only auto-run in verified terminals");
    run_open_command(editor_command(terminal, script))
}

/// Open a diagram preview page in the default browser: fixed `open <path>`
/// argv, never a shell command. The path is app-generated (see
/// [`write_preview_html`]); a missing or non-file target fails closed.
/// Fire-and-forget like transcript link opens: `open` is reaped on a
/// short-lived thread so a cold browser launch never blocks the UI.
pub(crate) fn open_in_browser(path: &Path) -> Result<(), String> {
    if !fs::metadata(path).map(|metadata| metadata.is_file()).unwrap_or(false) {
        return Err(rust_i18n::t!("app.markdown.diagram_open_failed").to_string());
    }
    let mut child = browser_command(path).spawn().map_err(|error| {
        rust_i18n::t!("app.markdown.diagram_open_failed_detail", detail = error.to_string()).to_string()
    })?;
    let _reaper = std::thread::spawn(move || drop(child.wait()));
    Ok(())
}

fn browser_command(path: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/open");
    let _command = command.arg(path);
    command
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

    #[test]
    fn only_terminal_app_runs_handoff_scripts() {
        assert!(terminal_runs_handoff_script(ExternalEditor::Terminal));
        for editor in [ExternalEditor::ITerm2, ExternalEditor::Ghostty] {
            assert!(!terminal_runs_handoff_script(editor), "{editor:?} stays on copy+open until verified");
        }
    }

    #[test]
    fn handoff_script_quotes_values_and_reports_failure() {
        let script = build_handoff_script(
            Path::new("/Applications/VT Code.app/Contents/Helpers/vtcode"),
            Path::new("/repo/workspace"),
            "sess-1",
        )
        .expect("plain paths encode");
        assert_eq!(
            script,
            "#!/bin/sh\ncd '/repo/workspace' && exec '/Applications/VT Code.app/Contents/Helpers/vtcode' --fork-session 'sess-1'\necho \"vtcode handoff failed with exit $?\"\nexit 1\n"
        );
        // Asymmetric pair: a quote inside the path becomes '\'' (close,
        // escaped quote, reopen), everything else untouched.
        let hostile = build_handoff_script(Path::new("/bin/vtcode"), Path::new("/sp ace/o'brien"), "sess-1")
            .expect("hostile paths encode");
        assert_eq!(
            hostile,
            "#!/bin/sh\ncd '/sp ace/o'\\''brien' && exec '/bin/vtcode' --fork-session 'sess-1'\necho \"vtcode handoff failed with exit $?\"\nexit 1\n"
        );
    }

    #[test]
    fn handoff_script_executes_literally_under_a_real_shell() {
        let temp = temp_workspace();
        // Hostile workspace: spaces, quote, command substitution, separator,
        // newline, unicode. If quoting breaks, `touch nope` fires or argv splits.
        let hostile_dir = temp.path().join("a b'c$(touch nope);d\nüñî");
        fs::create_dir(&hostile_dir).unwrap();
        // Stub vtcode records its cwd + argv through env-passed output files.
        // Separate files because the cwd itself contains a newline, which
        // would break line-based parsing of a shared record.
        let stub = temp.path().join("vtcode");
        let out = temp.path().join("argv.txt");
        let dir_out = temp.path().join("cwd.txt");
        fs::write(
            &stub,
            "#!/bin/sh\nprintf 'cwd=%s\\n' \"$PWD\" > \"$STUB_DIR\"\nprintf '%s\\n' \"$@\" > \"$STUB_OUT\"\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&stub, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let script = build_handoff_script(&stub, &hostile_dir, "sess-1").expect("hostile paths encode");
        let script_path = temp.path().join("handoff.command");
        fs::write(&script_path, &script).unwrap();
        let output = std::process::Command::new("sh")
            .arg(&script_path)
            .env("STUB_OUT", &out)
            .env("STUB_DIR", &dir_out)
            .output()
            .expect("sh runs the script");
        assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
        assert!(output.stdout.is_empty(), "the failure echo must not run on success");
        let recorded_cwd = fs::read_to_string(&dir_out).unwrap();
        let cwd = recorded_cwd
            .strip_prefix("cwd=")
            .and_then(|cwd| cwd.strip_suffix('\n'))
            .expect("cwd record");
        assert_eq!(
            vtcode_commons::paths::canonicalize(Path::new(cwd)).unwrap(),
            vtcode_commons::paths::canonicalize(&hostile_dir).unwrap(),
            "cd must land literally in the hostile directory"
        );
        let recorded_argv = fs::read_to_string(&out).unwrap();
        assert_eq!(recorded_argv.lines().collect::<Vec<_>>(), vec!["--fork-session", "sess-1"], "argv must not split");
        assert!(!temp.path().join("nope").exists(), "$(touch nope) must not execute");
    }

    #[test]
    fn handoff_script_reports_failure_instead_of_exiting_silently() {
        let temp = temp_workspace();
        // A workspace deleted between launch and run: cd fails, so the
        // script must print a visible failure and exit nonzero (keeping the
        // terminal window open) instead of landing nowhere silently.
        let missing = temp.path().join("deleted-workspace");
        let script = build_handoff_script(Path::new("/bin/vtcode"), &missing, "sess-1").expect("paths encode");
        let script_path = temp.path().join("handoff.command");
        fs::write(&script_path, &script).unwrap();
        let output = std::process::Command::new("sh")
            .arg(&script_path)
            .output()
            .expect("sh runs the script");
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("vtcode handoff failed"),
            "stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    #[test]
    fn handoff_script_rejects_non_utf8_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(b"/tmp/\xff\xfe".to_vec());
        assert!(build_handoff_script(Path::new("/bin/vtcode"), Path::new(&raw), "sess-1").is_none());
        assert!(build_handoff_script(Path::new(&raw), Path::new("/tmp/ws"), "sess-1").is_none());
    }

    #[test]
    fn preview_html_files_are_owner_only_and_unique() {
        struct RemoveOnDrop(PathBuf);
        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                drop(fs::remove_file(&self.0));
            }
        }
        let first = write_preview_html("<html></html>").expect("first write");
        let _first_guard = RemoveOnDrop(first.clone());
        let second = write_preview_html("<html></html>").expect("second write");
        let _second_guard = RemoveOnDrop(second.clone());
        assert_ne!(first, second, "concurrent previews must not share a path");
        assert_eq!(first.extension().and_then(|ext| ext.to_str()), Some("html"));
        assert_eq!(fs::read_to_string(&first).unwrap(), "<html></html>");
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&first).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(
            first.file_name().unwrap().to_string_lossy().starts_with(PREVIEW_TEMP_PREFIX),
            "prefix keeps previews apart from handoff scripts under one counter"
        );
    }

    #[test]
    fn stale_preview_policy_expires_at_the_ttl_boundary() {
        let ttl = std::time::Duration::from_secs(24 * 60 * 60);
        assert!(!preview_is_stale(std::time::Duration::ZERO, ttl));
        let just_under = ttl.checked_sub(std::time::Duration::from_nanos(1)).expect("ttl is large");
        assert!(!preview_is_stale(just_under, ttl));
        assert!(preview_is_stale(ttl, ttl));
        assert!(preview_is_stale(ttl + std::time::Duration::from_nanos(1), ttl));
    }

    #[test]
    fn sweep_removes_only_stale_previews_in_isolation() {
        let workspace = temp_workspace();
        let stale_a = workspace.path().join("vtcode-diagram-1-0.html");
        let stale_b = workspace.path().join("vtcode-diagram-2-0.html");
        let foreign = workspace.path().join("other.txt");
        let not_a_file = workspace.path().join("vtcode-diagram-subdir");
        fs::write(&stale_a, "<html></html>").unwrap();
        fs::write(&stale_b, "<html></html>").unwrap();
        fs::write(&foreign, "x").unwrap();
        fs::create_dir(&not_a_file).unwrap();
        // Zero TTL makes every measurable age stale, deterministically.
        assert_eq!(sweep_previews_older_than(workspace.path(), std::time::Duration::ZERO), 2);
        assert!(!stale_a.exists() && !stale_b.exists(), "stale previews removed");
        assert!(foreign.exists(), "foreign files untouched");
        assert!(not_a_file.exists(), "directories untouched");
    }

    #[test]
    fn sweep_keeps_fresh_previews_in_shared_temp() {
        struct RemoveOnDrop(PathBuf);
        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                drop(fs::remove_file(&self.0));
            }
        }
        let path = write_preview_html("<html></html>").expect("write");
        let _guard = RemoveOnDrop(path.clone());
        let _removed = sweep_stale_previews();
        assert!(path.exists(), "fresh previews survive the production sweep");
    }

    #[test]
    fn browser_open_argv_is_a_fixed_open_of_the_app_generated_path() {
        let path = PathBuf::from("/tmp/vtcode-diagram-1-0.html");
        let command = browser_command(&path);
        assert_eq!(command.get_program(), "/usr/bin/open");
        assert_eq!(command.get_args().collect::<Vec<_>>(), vec![path.as_os_str()]);
    }

    #[test]
    fn browser_open_rejects_missing_and_non_file_targets_without_launching() {
        // The success path spawns a real browser, so only the fail-closed
        // guards are pinned here.
        let workspace = temp_workspace();
        assert!(open_in_browser(&workspace.path().join("missing.html")).is_err());
        assert!(open_in_browser(workspace.path()).is_err(), "directories are rejected");
    }

    #[test]
    fn handoff_script_files_are_owner_only_and_unique() {
        struct RemoveOnDrop(PathBuf);
        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                drop(fs::remove_file(&self.0));
            }
        }
        let first = write_handoff_script("#!/bin/sh\necho hi\n").expect("first write");
        let _first_guard = RemoveOnDrop(first.clone());
        let second = write_handoff_script("#!/bin/sh\necho hi\n").expect("second write");
        let _second_guard = RemoveOnDrop(second.clone());
        assert_ne!(first, second, "concurrent handoffs must not share a path");
        assert_eq!(first.extension().and_then(|ext| ext.to_str()), Some("command"));
        assert_eq!(fs::read_to_string(&first).unwrap(), "#!/bin/sh\necho hi\n");
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&first).unwrap().permissions().mode() & 0o777, 0o700);
        }
    }
}
