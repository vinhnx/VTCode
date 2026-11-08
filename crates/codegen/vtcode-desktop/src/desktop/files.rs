//! Bounded workspace file listing and previews for the sidebar, plus the
//! confined mutation helpers (`mutate`) behind the file context menu.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::strings::bundle;

mod launch;
mod model;
mod mutate;

pub(super) use launch::{
    CLI_FORK_SESSION_FLAG, build_handoff_script, open_in_browser, open_in_editor, open_script_in_terminal,
    open_terminal_at, reveal_in_finder, terminal_runs_handoff_script, write_handoff_script, write_preview_html,
};
pub(super) use model::{SEARCH_MAX_HITS, SNAPSHOT_MAX_ENTRIES, fuzzy_search, fuzzy_search_files, scan_snapshot};
#[cfg(test)]
pub(super) use mutate::trash_entry_into;
pub(super) use mutate::{rename_entry, trash_entry, validated_file_name};

/// Preview read bound; a `usize` so buffer sizing never truncates.
const MAX_PREVIEW_BYTES: usize = 64 * 1024;
/// Image read bound for thumbnails and the preview viewer.
const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;

/// Canonicalize a workspace candidate and confirm it is a directory.
/// Call from a GPUI background task because both operations access the filesystem.
pub(super) fn canonicalize_workspace_folder(path: &Path) -> Option<PathBuf> {
    let canonical = vtcode_commons::paths::canonicalize(path).ok()?;
    canonical.is_dir().then_some(canonical)
}

/// One workspace entry for the sidebar listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileEntry {
    /// Path relative to the workspace root, using `/` separators.
    pub rel_path: String,
    /// Nesting depth below the root; the root itself is depth 0.
    pub depth: usize,
    /// Whether this entry is a directory.
    pub is_dir: bool,
}

/// A bounded file preview.
pub(super) struct FilePreview {
    /// Display path: workspace-relative, or absolute for files read from
    /// anywhere else on disk.
    pub rel_path: String,
    /// Decoded preview text; empty for image previews.
    pub text: String,
    /// Whether the file is larger than the preview bound.
    pub truncated: bool,
    /// Decoded-ready image bytes for image files; `None` for text previews.
    pub image: Option<Arc<gpui_kit::Image>>,
    /// Markdown previews start rendered; the header toggle flips to the raw
    /// source surface. Owned per preview so every open starts rendered.
    pub show_source: bool,
    /// The source surface is read-only until the Edit toggle arms it; edits
    /// persist only through Save. Owned per preview like `show_source`.
    pub editing: bool,
}

/// Whether a workspace-relative path points at an image the app can render
/// and the LLM providers accept: the commons allowlist (png/jpg/jpeg/gif/webp)
/// is the single source of truth, mirroring `ContentPart::validate_image`.
pub(super) fn is_image_path(rel_path: &str) -> bool {
    vtcode_commons::image::has_supported_image_extension(Path::new(rel_path))
}

/// Whether a workspace-relative path points at a markdown document the file
/// preview renders through the shared markdown viewer. Mirrors the
/// `md | markdown` pair `ast_grep` uses for language detection.
pub(super) fn is_markdown_path(rel_path: &str) -> bool {
    rel_path
        .rsplit('.')
        .next()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown"))
}

/// Map a provider-supported image extension to the renderer's decoders.
fn image_format(rel_path: &str) -> Option<gpui_kit::ImageFormat> {
    if !is_image_path(rel_path) {
        return None;
    }
    let extension = rel_path.rsplit('.').next()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some(gpui_kit::ImageFormat::Png),
        "jpg" | "jpeg" => Some(gpui_kit::ImageFormat::Jpeg),
        "gif" => Some(gpui_kit::ImageFormat::Gif),
        "webp" => Some(gpui_kit::ImageFormat::Webp),
        _ => None,
    }
}

/// Resolve one file for reading or launching. Relative paths stay confined
/// to the workspace (symlink escapes fail closed); absolute paths — which
/// only arrive from explicit user drops, pastes, or attachment chips —
/// resolve directly so files from anywhere can attach.
fn resolve_attachment_path(root: &Path, path: &Path) -> Result<PathBuf, ()> {
    if path.is_absolute() {
        vtcode_commons::paths::canonicalize(path).map_err(|_error| ())
    } else {
        vtcode_commons::paths::resolve_workspace_path(root, path).map_err(|_error| ())
    }
}

/// Read a bounded image file for thumbnails and the preview viewer, returned
/// ready for the renderer to decode.
pub(super) fn read_image(root: &Path, rel_path: &str) -> Option<Arc<gpui_kit::Image>> {
    let format = image_format(rel_path)?;
    let path = resolve_attachment_path(root, Path::new(rel_path)).ok()?;
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES as u64 {
        return None;
    }
    Some(Arc::new(gpui_kit::Image::from_bytes(format, fs::read(&path).ok()?)))
}

/// Read the current git branch for a workspace, walking up to the repository
/// root like git does. Detached HEAD yields a short commit hash. Worktrees
/// (`.git` as a file) follow their `gitdir` pointer. Bounded and read-only.
pub(super) fn git_branch(workspace: &Path) -> Option<String> {
    let mut dir = Some(workspace);
    for _ in 0..10 {
        let Some(current) = dir else { break };
        let dot_git = current.join(".git");
        if dot_git.is_dir() {
            return parse_branch_head(&dot_git);
        }
        if dot_git.is_file() {
            // Worktree/submodule pointer: `gitdir: <path>`.
            let pointer = fs::read_to_string(&dot_git).ok()?;
            let target = pointer.strip_prefix("gitdir:")?.trim();
            return parse_branch_head(&current.join(target));
        }
        dir = current.parent();
    }
    None
}

fn parse_branch_head(git_dir: &Path) -> Option<String> {
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref: refs/heads/") {
        return Some(reference.trim().to_string());
    }
    if head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(head.chars().take(7).collect());
    }
    None
}

/// Read a bounded text preview of one file. Returns `None` for missing,
/// oversized-binary, or non-UTF-8 files.
pub(super) fn read_preview(root: &Path, rel_path: &str) -> Option<FilePreview> {
    let path = resolve_attachment_path(root, Path::new(rel_path)).ok()?;
    if !fs::metadata(&path).ok()?.is_file() {
        return None;
    }
    let file = fs::File::open(&path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    // Read one extra byte to detect truncation even if the file grows after metadata.
    let mut bytes = Vec::new();
    let _read = file.take((MAX_PREVIEW_BYTES + 1) as u64).read_to_end(&mut bytes).ok()?;
    let truncated = bytes.len() > MAX_PREVIEW_BYTES;
    bytes.truncate(MAX_PREVIEW_BYTES);
    if bytes.contains(&0_u8) {
        return None;
    }
    // A valid UTF-8 character may straddle the preview boundary.
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) if truncated && error.error_len().is_none() => {
            std::str::from_utf8(bytes.get(..error.valid_up_to())?).ok()?
        }
        Err(_) => return None,
    };
    Some(FilePreview {
        rel_path: rel_path.to_string(),
        text: text.to_string(),
        truncated,
        image: None,
        show_source: false,
        editing: false,
    })
}

/// Persist edited preview text back to the previewed file. The caller owns
/// dirty tracking and must refuse truncated previews (their editor holds
/// only the head, so writing would destroy the tail); this only resolves,
/// writes, and reports a display-ready error. Call from a background task.
///
/// The target resolves through the same confinement boundary as the read
/// path, so traversal and symlink escapes fail closed and a file deleted
/// after opening errors instead of being recreated. Absolute previews —
/// which only arrive from explicit user attachments — resolve directly,
/// mirroring [`read_preview`].
pub(super) fn write_preview_text(root: &Path, rel_path: &str, text: &str) -> Result<(), String> {
    let path = resolve_attachment_path(root, Path::new(rel_path))
        .map_err(|()| format!("Could not save \"{rel_path}\": it is outside the workspace or no longer exists"))?;
    fs::write(&path, text).map_err(|error| format!("Could not save \"{rel_path}\": {error}"))
}

/// External applications supported by the file preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExternalEditor {
    System,
    VsCode,
    Sublime,
    Xcode,
    AndroidStudio,
    Finder,
    Terminal,
    ITerm2,
    Ghostty,
}

/// How `/usr/bin/open` receives the resolved path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenAction {
    /// Open the file itself.
    Open,
    /// Reveal the file without opening it.
    Reveal,
    /// Open the containing directory (terminal emulators).
    OpenDirectory,
}

impl ExternalEditor {
    #[cfg(test)]
    const ALL: [Self; 9] = [
        Self::System,
        Self::VsCode,
        Self::Sublime,
        Self::Xcode,
        Self::AndroidStudio,
        Self::Finder,
        Self::Terminal,
        Self::ITerm2,
        Self::Ghostty,
    ];

    /// Editors located by scanning the application directories for their
    /// bundle identifiers.
    pub(super) const DETECTABLE: [Self; 6] = [
        Self::VsCode,
        Self::Sublime,
        Self::Xcode,
        Self::AndroidStudio,
        Self::ITerm2,
        Self::Ghostty,
    ];

    /// Destinations needing no detection: the system default plus macOS apps
    /// that ship on the sealed system volume and cannot be uninstalled.
    #[cfg(test)]
    pub(super) const ALWAYS_AVAILABLE: [Self; 3] = [Self::System, Self::Finder, Self::Terminal];

    /// The bundle identifier used both to detect the installation and to
    /// launch it, or `None` for destinations that need neither.
    const fn bundle_id(self) -> Option<&'static str> {
        match self {
            Self::System | Self::Finder => None,
            Self::Terminal => Some("com.apple.Terminal"),
            Self::VsCode => Some("com.microsoft.VSCode"),
            Self::Sublime => Some("com.sublimetext.4"),
            Self::Xcode => Some("com.apple.dt.Xcode"),
            Self::AndroidStudio => Some("com.google.android.studio"),
            Self::ITerm2 => Some("com.googlecode.iterm2"),
            Self::Ghostty => Some("com.mitchellh.ghostty"),
        }
    }

    const fn open_action(self) -> OpenAction {
        match self {
            Self::Finder => OpenAction::Reveal,
            Self::Terminal | Self::ITerm2 | Self::Ghostty => OpenAction::OpenDirectory,
            Self::System | Self::VsCode | Self::Sublime | Self::Xcode | Self::AndroidStudio => OpenAction::Open,
        }
    }
}

/// Maximum directory entries inspected per application root during detection.
const MAX_APP_SCAN_ENTRIES: usize = 512;

/// Application directories scanned for installed editors.
fn app_roots() -> Vec<PathBuf> {
    // Test hook: GUI tests redirect detection at deterministic fake roots.
    #[cfg(test)]
    if let Some(roots) = TEST_APP_ROOTS.with(|roots| roots.borrow().clone()) {
        return roots;
    }
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }
    roots
}

/// Detect installed editors by the bundle identifier registered in each
/// application's `Info.plist`. Bounded per root with early exit once every
/// known editor is found; unreadable or unparseable bundles are skipped.
pub(super) fn detect_installed_editors() -> Vec<ExternalEditor> {
    scan_app_roots(&app_roots())
}

/// Maximum `Info.plist` bytes read per bundle during detection.
const MAX_INFO_PLIST_BYTES: usize = 256 * 1024;

fn bundle_id_of(app_dir: &Path) -> Option<String> {
    let file = fs::File::open(app_dir.join(bundle::CONTENTS_DIR).join(bundle::INFO_PLIST)).ok()?;
    let mut bytes = Vec::new();
    let _read = file.take(MAX_INFO_PLIST_BYTES as u64).read_to_end(&mut bytes).ok()?;
    let value: plist::Value = plist::from_bytes(&bytes).ok()?;
    value.into_dictionary()?.remove("CFBundleIdentifier")?.into_string()
}

fn scan_app_roots(roots: &[PathBuf]) -> Vec<ExternalEditor> {
    let mut found = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten().take(MAX_APP_SCAN_ENTRIES) {
            if found.len() == ExternalEditor::DETECTABLE.len() {
                break;
            }
            // Symlinked installs count; dangling links fail closed on the plist read.
            let is_bundle = entry
                .file_type()
                .map(|kind| kind.is_dir() || kind.is_symlink())
                .unwrap_or(false)
                && entry.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app"));
            if !is_bundle {
                continue;
            }
            if let Some(id) = bundle_id_of(&entry.path())
                && let Some(editor) = ExternalEditor::DETECTABLE
                    .into_iter()
                    .find(|editor| editor.bundle_id() == Some(id.as_str()))
                && !found.contains(&editor)
            {
                found.push(editor);
            }
        }
        if found.len() == ExternalEditor::DETECTABLE.len() {
            break;
        }
    }
    // Canonical order regardless of filesystem iteration order.
    ExternalEditor::DETECTABLE
        .into_iter()
        .filter(|editor| found.contains(editor))
        .collect()
}

#[cfg(test)]
std::thread_local! {
    static TEST_APP_ROOTS: std::cell::RefCell<Option<Vec<PathBuf>>> = const { std::cell::RefCell::new(None) };
}

/// Redirect install detection at fake roots for one GUI test. Hold the
/// returned guard for the test's duration; dropping it restores real roots.
#[cfg(test)]
#[must_use = "hold the guard or the override leaks into other tests"]
pub(super) fn set_test_app_roots(roots: Vec<PathBuf>) -> TestAppRootsGuard {
    TEST_APP_ROOTS.with(|slot| *slot.borrow_mut() = Some(roots));
    TestAppRootsGuard { _private: () }
}

/// RAII guard restoring real application roots on drop, even on failure.
#[cfg(test)]
pub(super) struct TestAppRootsGuard {
    _private: (),
}

#[cfg(test)]
impl Drop for TestAppRootsGuard {
    fn drop(&mut self) {
        clear_test_app_roots();
    }
}

/// Restore real application roots. Tests that override detection must pair
/// every override with this call (an RAII guard is safest) so it never leaks
/// into another test on a reused worker thread.
#[cfg(test)]
pub(super) fn clear_test_app_roots() {
    TEST_APP_ROOTS.with(|slot| *slot.borrow_mut() = None);
}

/// Write a minimal `.app` bundle registering `bundle_id` for detection tests.
#[cfg(test)]
pub(super) fn write_test_bundle(root: &Path, name: &str, bundle_id: &str) {
    let contents = root.join(name).join("Contents");
    fs::create_dir_all(&contents).expect("bundle contents");
    fs::write(
        contents.join("Info.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{bundle_id}</string></dict></plist>"
        ),
    )
    .expect("bundle plist");
}

/// Payload the platform receives when a file-tree drag leaves the window and
/// is promoted to a native session: the workspace entry as an absolute path,
/// paired with its directory flag so Finder drops behave correctly.
pub(super) fn external_drag_payload(
    root: &Path,
    rel_path: &str,
    is_dir: bool,
) -> Option<gpui_kit::ExternalDragPayload> {
    if rel_path.is_empty() {
        return None;
    }
    Some(gpui_kit::ExternalDragPayload::Files(gpui_kit::FileDragPaths::new([(root.join(rel_path), is_dir)])))
}

/// Attach complete text only; the preview's truncation is never silently
/// submitted. Images attach by path alone (the agent reads them itself), so
/// non-UTF-8 image formats are first-class attachments instead of rejections.
/// Workspace files keep their relative label; files from anywhere else
/// attach under their absolute path.
pub(super) fn read_attachment(root: &Path, path: &Path) -> Result<vtcode_desktop_protocol::TurnAttachment, String> {
    let root = vtcode_commons::paths::canonicalize(root).map_err(|error| error.to_string())?;
    let resolved =
        resolve_attachment_path(&root, path).map_err(|_error| rust_i18n::t!("app.files.missing").to_string())?;
    let relative = resolved
        .strip_prefix(&root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| resolved.to_string_lossy().replace('\\', "/"));
    if is_image_path(&relative) {
        return Ok(vtcode_desktop_protocol::TurnAttachment { path: relative, text: String::new() });
    }
    // Binary image formats no vision provider accepts are rejected up front
    // instead of falling through to a text read that would produce mojibake.
    let lower = relative.to_ascii_lowercase();
    if vtcode_commons::image::UNSUPPORTED_BINARY_IMAGE_EXTENSIONS
        .iter()
        .any(|extension| lower.ends_with(&format!(".{extension}")))
    {
        return Err(format!("\"{relative}\" is a binary image format (BMP/TIFF) that the providers do not accept."));
    }
    let preview = read_preview(&root, &relative)
        .filter(|preview| !preview.truncated)
        .ok_or_else(|| format!("“{relative}” must be a UTF-8 text file no larger than 64 KiB."))?;
    Ok(vtcode_desktop_protocol::TurnAttachment { path: relative, text: preview.text })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("temp dir")
    }

    #[test]
    fn image_paths_map_to_renderable_formats_only() {
        for path in ["shot.png", "photo.JPG", "anim.gif", "render.webp"] {
            assert!(is_image_path(path), "{path} must be an image");
        }
        // BMP/TIFF are binary image formats no vision provider accepts; they
        // must not pass the image gate (read_attachment rejects them with a
        // dedicated notice instead).
        for path in [
            "icon.ico",
            "legacy.bmp",
            "scan.tiff",
            "notes.md",
            "main.rs",
            "archive.zip",
            "noext",
        ] {
            assert!(!is_image_path(path), "{path} must not be an image");
        }
    }

    #[test]
    fn markdown_paths_route_to_the_shared_viewer() {
        for path in ["notes.md", "README.MD", "guide.markdown", "docs/plan.Markdown"] {
            assert!(is_markdown_path(path), "{path} must be markdown");
        }
        for path in ["main.rs", "notes.mdx", "archive.zip", "noext", "shot.png", "mddoc"] {
            assert!(!is_markdown_path(path), "{path} must not be markdown");
        }
    }

    #[test]
    fn binary_image_formats_reject_with_a_clear_notice() {
        let workspace = temp_workspace();
        fs::write(workspace.path().join("legacy.bmp"), b"BM\x00binary").unwrap();
        let error = read_attachment(workspace.path(), Path::new("legacy.bmp")).expect_err("bmp must reject");
        assert!(error.contains("binary image format"), "the notice must name the binary-format rejection: {error}");
    }

    #[test]
    fn image_attachments_carry_the_path_without_text_content() {
        let workspace = temp_workspace();
        let image = workspace.path().join("shot.png");
        fs::write(&image, b"not-really-a-png").unwrap();
        let attachment = read_attachment(workspace.path(), Path::new("shot.png")).expect("image attaches");
        assert_eq!(attachment.path, "shot.png");
        assert!(attachment.text.is_empty(), "images attach by path, never inline text");
    }

    #[test]
    fn outside_files_attach_under_their_absolute_path() {
        use std::os::unix::fs::symlink;
        let workspace = temp_workspace();
        let outside = temp_workspace();
        let file = outside.path().join("outside.md");
        fs::write(&file, "shared").unwrap();
        // Asymmetric pair: the same bytes attach relatively inside the
        // workspace and absolutely outside it.
        fs::write(workspace.path().join("inside.md"), "shared").unwrap();
        let inside = read_attachment(workspace.path(), Path::new("inside.md")).expect("inside attaches");
        assert_eq!(inside.path, "inside.md");
        assert_eq!(inside.text, "shared");
        let canonical = vtcode_commons::paths::canonicalize(&file).expect("canonical fixture");
        let attached = read_attachment(workspace.path(), &file).expect("outside attaches");
        assert_eq!(attached.path, canonical.to_string_lossy().replace('\\', "/"));
        assert_eq!(attached.text, "shared");
        // A relative symlink escape still fails closed: only explicit
        // absolute paths leave the workspace.
        symlink(outside.path(), workspace.path().join("escape")).unwrap();
        assert!(read_attachment(workspace.path(), Path::new("escape/outside.md")).is_err());
        assert!(read_attachment(workspace.path(), &outside.path().join("gone.md")).is_err());
    }

    #[test]
    fn read_image_bounds_and_rejects_non_images() {
        let workspace = temp_workspace();
        let root = workspace.path();
        fs::write(root.join("tiny.png"), [0x89, b'P', b'N', b'G']).unwrap();
        let image = read_image(root, "tiny.png").expect("small image reads");
        assert!(Arc::strong_count(&image) >= 1);

        let big: Vec<u8> = vec![0u8; MAX_IMAGE_BYTES + 1];
        fs::write(root.join("huge.png"), &big).unwrap();
        assert!(read_image(root, "huge.png").is_none(), "oversized images must not load");

        assert!(read_image(root, "notes.txt").is_none(), "non-images have no format");
        fs::write(root.join("notes.txt"), "text").unwrap();
        assert!(read_image(root, "notes.txt").is_none(), "text files are not images");
    }

    #[test]
    fn external_drag_payload_maps_workspace_entries_to_absolute_paths() {
        let workspace = temp_workspace();
        let root = workspace.path();
        let gpui_kit::ExternalDragPayload::Files(paths) =
            external_drag_payload(root, "src/main.rs", false).expect("file payload");
        let entries = paths.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], (root.join("src/main.rs"), false));

        let gpui_kit::ExternalDragPayload::Files(paths) =
            external_drag_payload(root, "src", true).expect("directory payload");
        assert_eq!(paths.entries()[0], (root.join("src"), true), "directory flag must be preserved");

        assert!(external_drag_payload(root, "", false).is_none(), "empty paths must not drag");
    }

    #[test]
    fn editor_catalog_partitions_always_available_and_detectable_targets() {
        assert_eq!(ExternalEditor::ALL.len(), 9, "extend this test when adding an editor");
        for editor in ExternalEditor::ALL {
            let always = ExternalEditor::ALWAYS_AVAILABLE.contains(&editor);
            let detectable = ExternalEditor::DETECTABLE.contains(&editor);
            assert!(always ^ detectable, "{editor:?} must be exclusively detectable or always available");
            if detectable {
                assert!(editor.bundle_id().is_some(), "{editor:?} needs an identifier to detect");
            }
        }
        assert_eq!(ExternalEditor::DETECTABLE.len(), 6);
        assert_eq!(ExternalEditor::ALWAYS_AVAILABLE.len(), 3);
        assert_eq!(ExternalEditor::System.bundle_id(), None);
        assert_eq!(ExternalEditor::Finder.bundle_id(), None);
        for editor in [
            ExternalEditor::Terminal,
            ExternalEditor::ITerm2,
            ExternalEditor::Ghostty,
        ] {
            assert_eq!(editor.open_action(), OpenAction::OpenDirectory, "{editor:?} opens a directory");
        }
        assert_eq!(ExternalEditor::Finder.open_action(), OpenAction::Reveal);
        for editor in [
            ExternalEditor::System,
            ExternalEditor::VsCode,
            ExternalEditor::Sublime,
            ExternalEditor::Xcode,
            ExternalEditor::AndroidStudio,
        ] {
            assert_eq!(editor.open_action(), OpenAction::Open, "{editor:?} opens the file");
        }
    }

    #[test]
    fn install_detection_matches_bundle_identifiers_across_roots_in_canonical_order() {
        let first = temp_workspace();
        let second = temp_workspace();
        write_test_bundle(first.path(), "Ghostty.app", "com.mitchellh.ghostty");
        // Detection keys on the registered identifier, not the directory name.
        write_test_bundle(first.path(), "Renamed Code.app", "com.microsoft.VSCode");
        write_test_bundle(second.path(), "iTerm.app", "com.googlecode.iterm2");
        // Near misses that must not match: a file in place of a bundle, an
        // unknown identifier, malformed XML, and a bundle without a plist.
        fs::write(first.path().join("Xcode.app"), "not a directory").unwrap();
        write_test_bundle(first.path(), "Other.app", "com.example.other");
        let broken = second.path().join("Broken.app").join("Contents");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("Info.plist"), "not xml{{{").unwrap();
        fs::create_dir(second.path().join("Bare.app")).unwrap();
        // Symlinked installs count by their target's identifier; dangling and
        // file-target links fail closed. The real bundle lives outside both
        // scanned roots so only the link can surface it.
        use std::os::unix::fs::symlink;
        let hidden = temp_workspace();
        write_test_bundle(hidden.path(), "Real Studio.app", "com.google.android.studio");
        symlink(hidden.path().join("Real Studio.app"), second.path().join("Linked.app")).unwrap();
        symlink(first.path().join("missing-target"), second.path().join("Dangling.app")).unwrap();
        symlink(first.path().join("Xcode.app"), second.path().join("FileLink.app")).unwrap();

        let found = scan_app_roots(&[first.path().to_path_buf(), second.path().to_path_buf()]);
        assert_eq!(
            found,
            vec![
                ExternalEditor::VsCode,
                ExternalEditor::AndroidStudio,
                ExternalEditor::ITerm2,
                ExternalEditor::Ghostty,
            ]
        );

        let missing = scan_app_roots(&[first.path().join("no-such-dir")]);
        assert!(missing.is_empty(), "missing roots scan as empty");
        let empty = scan_app_roots(&[]);
        assert!(empty.is_empty(), "no roots means no editors");
    }

    #[test]
    fn workspace_folder_validation_canonicalizes_directories_only() {
        let workspace = temp_workspace();
        let nested = workspace.path().join("nested");
        fs::create_dir(&nested).expect("create nested directory");
        fs::write(workspace.path().join("file.txt"), b"text").expect("write file");

        let candidate = nested.join("..").join("nested");
        let expected = vtcode_commons::paths::canonicalize(&nested).unwrap();
        assert_eq!(canonicalize_workspace_folder(&candidate), Some(expected));
        assert_eq!(canonicalize_workspace_folder(&workspace.path().join("file.txt")), None);
        assert_eq!(canonicalize_workspace_folder(&workspace.path().join("missing")), None);
    }

    #[test]
    fn preview_reads_text_and_truncates_oversized_files() {
        let workspace = temp_workspace();
        let root = workspace.path();
        fs::write(root.join("notes.md"), "short notes").expect("write");
        let large = "y".repeat(MAX_PREVIEW_BYTES + 10);
        fs::write(root.join("large.log"), large).expect("write");
        fs::write(root.join("binary.bin"), [0_u8, 1, 2]).expect("write");

        let preview = read_preview(root, "notes.md").expect("text preview");
        assert_eq!(preview.text, "short notes");
        assert!(!preview.truncated);

        let preview = read_preview(root, "large.log").expect("large preview");
        assert!(preview.truncated);
        assert_eq!(preview.text.len(), MAX_PREVIEW_BYTES);

        assert!(read_preview(root, "binary.bin").is_none(), "binary files have no text preview");
        assert!(read_preview(root, "missing.txt").is_none(), "missing files have no preview");
    }

    #[test]
    fn previews_and_attachments_reject_workspace_escapes() {
        use std::os::unix::fs::symlink;

        let workspace = temp_workspace();
        let outside = temp_workspace();
        fs::write(outside.path().join("secret.txt"), "private").unwrap();
        symlink(outside.path(), workspace.path().join("escape")).unwrap();
        fs::write(workspace.path().join("inside.txt"), "allowed").unwrap();
        symlink(workspace.path().join("inside.txt"), workspace.path().join("alias.txt")).unwrap();

        let absolute = outside.path().join("secret.txt");
        let traversal = format!("../{}/secret.txt", outside.path().file_name().unwrap().to_str().unwrap());
        // Relative escapes fail closed; explicit absolute paths are user
        // intent and resolve directly.
        for path in [traversal.as_str(), "escape/secret.txt"] {
            assert!(read_preview(workspace.path(), path).is_none(), "reject {path}");
            assert!(read_attachment(workspace.path(), Path::new(path)).is_err(), "reject {path}");
        }
        assert_eq!(read_preview(workspace.path(), absolute.to_str().unwrap()).unwrap().text, "private");
        assert_eq!(read_preview(workspace.path(), "alias.txt").unwrap().text, "allowed");
        let attachment = read_attachment(workspace.path(), &workspace.path().join("alias.txt")).unwrap();
        assert_eq!(attachment.path, "inside.txt");
        assert_eq!(attachment.text, "allowed");
    }

    #[test]
    fn attachments_require_complete_utf8_text_at_the_size_boundary() {
        let workspace = temp_workspace();
        let root = workspace.path();
        fs::write(root.join("exact.txt"), "x".repeat(MAX_PREVIEW_BYTES)).unwrap();
        fs::write(root.join("large.txt"), "x".repeat(MAX_PREVIEW_BYTES + 1)).unwrap();
        fs::write(root.join("invalid.txt"), [0xff, 0xfe]).unwrap();
        fs::write(root.join("binary.txt"), [b'a', 0, b'b']).unwrap();
        assert_eq!(read_attachment(root, Path::new("exact.txt")).unwrap().text.len(), MAX_PREVIEW_BYTES);
        for path in ["large.txt", "invalid.txt", "binary.txt"] {
            assert!(read_attachment(root, Path::new(path)).is_err(), "reject {path}");
        }
        let boundary = format!("{}é", "x".repeat(MAX_PREVIEW_BYTES - 1));
        fs::write(root.join("unicode.txt"), boundary).unwrap();
        let preview = read_preview(root, "unicode.txt").unwrap();
        assert!(preview.truncated);
        assert_eq!(preview.text, "x".repeat(MAX_PREVIEW_BYTES - 1));
        assert!(read_attachment(root, Path::new("unicode.txt")).is_err());
    }

    #[test]
    fn git_branch_reads_refs_detached_heads_and_worktrees() {
        let workspace = temp_workspace();
        let root = workspace.path();
        let git = root.join(".git");
        fs::create_dir_all(&git).expect("git dir");

        fs::write(git.join("HEAD"), "ref: refs/heads/feature/dark-mode\n").expect("head");
        assert_eq!(git_branch(root).as_deref(), Some("feature/dark-mode"));

        fs::write(git.join("HEAD"), "0123456789abcdef0123456789abcdef01234567").expect("head");
        assert_eq!(git_branch(root).as_deref(), Some("0123456"));

        fs::write(git.join("HEAD"), "ref: refs/remotes/origin/main").expect("head");
        assert_eq!(git_branch(root), None, "non-branch refs are not reported as branches");

        // A subdirectory still finds the repository root.
        let nested = root.join("crates/deep");
        fs::create_dir_all(&nested).expect("nested dir");
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n").expect("head");
        assert_eq!(git_branch(&nested).as_deref(), Some("main"));

        // Worktree pointer: `.git` is a file pointing at the real git dir.
        let worktree = temp_workspace();
        let real_git = worktree.path().join("real-git");
        fs::create_dir_all(&real_git).expect("git dir");
        fs::write(real_git.join("HEAD"), "ref: refs/heads/wt\n").expect("head");
        fs::write(worktree.path().join("code.rs"), b"x").expect("file");
        fs::write(worktree.path().join(".git"), format!("gitdir: {}\n", real_git.display())).expect("pointer");
        assert_eq!(git_branch(worktree.path()).as_deref(), Some("wt"));
        fs::write(worktree.path().join(".git"), "gitdir: real-git\n").expect("relative pointer");
        assert_eq!(git_branch(worktree.path()).as_deref(), Some("wt"));

        // No repository at all.
        let plain = temp_workspace();
        assert_eq!(git_branch(plain.path()), None);
    }
}
