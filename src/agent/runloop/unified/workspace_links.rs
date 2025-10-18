use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::slash_commands::WorkspaceDirectoryCommand;

#[derive(Clone)]
pub(crate) struct LinkedDirectory {
    pub(crate) original: PathBuf,
    pub(crate) link_path: PathBuf,
    pub(crate) display_path: String,
}

pub(crate) fn sanitize_alias_component(component: &str) -> String {
    let lowered = component.trim().to_ascii_lowercase();
    let mut sanitized = String::new();
    let mut last_was_dash = false;

    for ch in lowered.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch);
            last_was_dash = false;
        } else if matches!(ch, '-' | '_' | '.' | ' ' | '/') {
            if !last_was_dash && !sanitized.is_empty() {
                sanitized.push('-');
                last_was_dash = true;
            }
        }
    }

    let trimmed = sanitized.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "linked-dir".to_string()
    } else {
        trimmed
    }
}

pub(crate) fn create_directory_symlink(target: &Path, link: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).with_context(|| {
            format!(
                "failed to create symlink {} -> {}",
                link.display(),
                target.display()
            )
        })?;
    }

    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link).with_context(|| {
            format!(
                "failed to create symlink {} -> {}",
                link.display(),
                target.display()
            )
        })?;
    }

    Ok(())
}

pub(crate) fn remove_directory_symlink(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        if let Err(err) = fs::remove_file(path) {
            if err.kind() != ErrorKind::NotFound {
                return Err(err).with_context(|| {
                    format!("failed to remove directory link {}", path.display())
                });
            }
        }
    }

    #[cfg(windows)]
    {
        if let Err(err) = fs::remove_dir(path) {
            if err.kind() != ErrorKind::NotFound {
                return Err(err).with_context(|| {
                    format!("failed to remove directory link {}", path.display())
                });
            }
        }
    }

    Ok(())
}

pub(crate) fn handle_workspace_directory_command(
    renderer: &mut AnsiRenderer,
    workspace_root: &Path,
    command: WorkspaceDirectoryCommand,
    linked_directories: &mut Vec<LinkedDirectory>,
) -> Result<()> {
    match command {
        WorkspaceDirectoryCommand::List => {
            renderer.line(MessageStyle::Status, "Linked directories:")?;
            if linked_directories.is_empty() {
                renderer.line(MessageStyle::Info, "  (none)")?;
            } else {
                for (index, entry) in linked_directories.iter().enumerate() {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "  {}. {} → {}",
                            index + 1,
                            entry.display_path,
                            entry.original.display()
                        ),
                    )?;
                }
            }
            renderer.line(
                MessageStyle::Info,
                "Use /add-dir <path> to add or /add-dir --remove <alias> to detach.",
            )?;
            Ok(())
        }
        WorkspaceDirectoryCommand::Add(raw_paths) => {
            if raw_paths.is_empty() {
                return Ok(());
            }

            let link_root = workspace_root.join(".vtcode").join("external");
            fs::create_dir_all(&link_root).with_context(|| {
                format!("failed to prepare link directory {}", link_root.display())
            })?;

            for raw in raw_paths {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let candidate = PathBuf::from(trimmed);
                let resolved = if candidate.is_absolute() {
                    candidate
                } else {
                    workspace_root.join(candidate)
                };

                let canonical = match fs::canonicalize(&resolved) {
                    Ok(path) => path,
                    Err(err) => {
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to resolve '{}': {}", resolved.display(), err),
                        )?;
                        continue;
                    }
                };

                if !canonical.is_dir() {
                    renderer.line(
                        MessageStyle::Error,
                        &format!(
                            "Path '{}' is not a directory and cannot be linked.",
                            canonical.display()
                        ),
                    )?;
                    continue;
                }

                if linked_directories
                    .iter()
                    .any(|entry| entry.original == canonical)
                {
                    renderer.line(
                        MessageStyle::Info,
                        &format!("Directory already linked: {}", canonical.display()),
                    )?;
                    continue;
                }

                let alias_base = canonical
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(sanitize_alias_component)
                    .filter(|alias| !alias.is_empty())
                    .unwrap_or_else(|| format!("linked-dir-{}", linked_directories.len() + 1));

                let mut alias = alias_base.clone();
                let mut counter = 2usize;
                while linked_directories
                    .iter()
                    .any(|entry| entry.display_path.ends_with(&alias))
                    || link_root.join(&alias).exists()
                {
                    alias = format!("{}-{}", alias_base, counter);
                    counter += 1;
                }

                let link_path = link_root.join(&alias);
                if let Err(err) = create_directory_symlink(&canonical, &link_path) {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to link {}: {}", canonical.display(), err),
                    )?;
                    continue;
                }

                let display_path = format!(".vtcode/external/{}", alias);
                renderer.line(
                    MessageStyle::Info,
                    &format!("Linked {} as {}", canonical.display(), display_path),
                )?;
                renderer.line(
                    MessageStyle::Info,
                    "Access files in this directory using the linked path inside the workspace.",
                )?;

                linked_directories.push(LinkedDirectory {
                    original: canonical,
                    link_path,
                    display_path,
                });
            }

            Ok(())
        }
        WorkspaceDirectoryCommand::Remove(targets) => {
            if targets.is_empty() {
                renderer.line(
                    MessageStyle::Error,
                    "Usage: /add-dir --remove <alias|path> [more...]",
                )?;
                return Ok(());
            }

            let mut any_removed = false;
            for target in targets {
                match detach_linked_directory(renderer, &target, linked_directories) {
                    Ok(true) => {
                        any_removed = true;
                    }
                    Ok(false) => {}
                    Err(err) => {
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to remove '{}': {}", target, err),
                        )?;
                    }
                }
            }

            if !any_removed {
                renderer.line(
                    MessageStyle::Info,
                    "No matching linked directories were removed.",
                )?;
            }

            Ok(())
        }
    }
}

pub(crate) fn detach_linked_directory(
    renderer: &mut AnsiRenderer,
    target: &str,
    linked_directories: &mut Vec<LinkedDirectory>,
) -> Result<bool> {
    if linked_directories.is_empty() {
        renderer.line(MessageStyle::Info, "No linked directories to remove.")?;
        return Ok(false);
    }

    let normalized = target.trim();
    if normalized.is_empty() {
        return Ok(false);
    }

    let stripped = normalized
        .strip_prefix(".vtcode/external/")
        .unwrap_or(normalized)
        .to_string();

    let mut index_to_remove: Option<usize> = None;

    if let Ok(number) = stripped.parse::<usize>() {
        if number >= 1 && number <= linked_directories.len() {
            index_to_remove = Some(number - 1);
        }
    }

    if index_to_remove.is_none() {
        for (index, entry) in linked_directories.iter().enumerate() {
            if entry.display_path.ends_with(&stripped)
                || entry
                    .original
                    .to_string_lossy()
                    .eq_ignore_ascii_case(normalized)
                || entry
                    .link_path
                    .to_string_lossy()
                    .eq_ignore_ascii_case(normalized)
            {
                index_to_remove = Some(index);
                break;
            }
        }
    }

    if let Some(index) = index_to_remove {
        let entry = linked_directories.remove(index);
        remove_directory_symlink(&entry.link_path)?;
        renderer.line(
            MessageStyle::Info,
            &format!("Removed linked directory {}", entry.display_path),
        )?;
        return Ok(true);
    }

    renderer.line(
        MessageStyle::Error,
        &format!("No linked directory matched '{}'.", normalized),
    )?;
    Ok(false)
}
