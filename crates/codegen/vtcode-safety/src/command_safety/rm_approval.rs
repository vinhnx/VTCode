//! Context-aware approval checks for recursive `rm` commands.

use std::fs;
use std::path::{Path, PathBuf};
#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
use std::process::Command;

use super::dangerous_commands::unwrap_command_prefix;

/// Returns whether a recursive `rm` command requires approval based on its
/// target and the command working directory.
///
/// Relative targets are resolved from `working_directory`. A mounted root,
/// its immediate children, and known mount-layout roots require approval. If
/// mount data or target resolution is unavailable, the function fails closed.
/// Shell directory changes make relative target resolution uncertain and
/// therefore require approval for recursive deletes.
pub fn recursive_rm_targets_require_approval_in_directory(command: &[String], working_directory: &Path) -> bool {
    if !contains_recursive_rm(command) {
        return false;
    }
    let mount_points = mounted_filesystem_mount_points();
    recursive_rm_targets_require_approval_with_mount_points(command, working_directory, mount_points.as_deref())
}

fn recursive_rm_targets_require_approval_with_mount_points(
    command: &[String],
    working_directory: &Path,
    mount_points: Option<&[PathBuf]>,
) -> bool {
    if !contains_recursive_rm(command) {
        return false;
    }

    let Some(mount_points) = mount_points else {
        // Without a mount table, recursive deletes cannot be proven to avoid
        // mounted filesystems. Keep the decision on the approval path.
        return true;
    };

    recursive_rm_targets_are_sensitive(command, working_directory, mount_points)
}

#[derive(Debug, Default, PartialEq, Eq)]
struct RmArguments<'a> {
    recursive: bool,
    targets: Vec<&'a str>,
}

fn parse_rm_arguments(arguments: &[String]) -> RmArguments<'_> {
    let mut parsed = RmArguments::default();
    let mut end_of_options = false;

    for argument in arguments {
        if end_of_options {
            parsed.targets.push(argument);
            continue;
        }

        // Treat `--` as the end of option parsing, matching Codex's pinned
        // `rm_args_include_force_option` convention in
        // `codex-rs/shell-command/src/command_safety/is_dangerous_command.rs`
        // (commit `f29d55ff0697a88226e8a9811b3c007e5f719cba`).
        if argument == "--" {
            end_of_options = true;
        } else if argument == "--recursive" {
            parsed.recursive = true;
        } else if argument.starts_with("--") {
            // `rm` has no long option that consumes a separate argument.
        } else if let Some(flags) = argument
            .strip_prefix('-')
            .filter(|flags| !flags.is_empty() && !flags.starts_with('-'))
        {
            parsed.recursive |= flags.chars().any(|flag| matches!(flag, 'r' | 'R'));
        } else {
            parsed.targets.push(argument);
        }
    }

    parsed
}

fn contains_recursive_rm(command: &[String]) -> bool {
    let Some(command) = unwrap_command_prefix(command) else {
        return false;
    };
    let Some(executable) = command.first() else {
        return false;
    };

    let executable = Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(executable);
    if matches!(executable, "bash" | "sh" | "zsh") {
        let Some(flag) = command.get(1).map(String::as_str) else {
            return false;
        };
        let Some(script) = command.get(2) else {
            return false;
        };
        if matches!(flag, "-c" | "-lc" | "-ilc") {
            return parse_rm_shell_commands(script)
                .is_none_or(|commands| commands.iter().any(|nested| contains_recursive_rm(nested)));
        }
    }

    if executable != "rm" {
        return false;
    }

    command.get(1..).is_some_and(|arguments| {
        let parsed = parse_rm_arguments(arguments);
        parsed.recursive && !parsed.targets.is_empty()
    })
}

fn recursive_rm_targets_are_sensitive(command: &[String], working_directory: &Path, mount_points: &[PathBuf]) -> bool {
    recursive_rm_targets_are_sensitive_in_context(command, Some(working_directory), mount_points)
}

fn recursive_rm_targets_are_sensitive_in_context(
    command: &[String],
    working_directory: Option<&Path>,
    mount_points: &[PathBuf],
) -> bool {
    let Some(command) = unwrap_command_prefix(command) else {
        return false;
    };
    let Some(executable) = command.first() else {
        return false;
    };

    let executable = Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(executable);
    if matches!(executable, "bash" | "sh" | "zsh") {
        let Some(flag) = command.get(1).map(String::as_str) else {
            return false;
        };
        let Some(script) = command.get(2) else {
            return false;
        };
        if matches!(flag, "-c" | "-lc" | "-ilc") {
            let Some(commands) = parse_rm_shell_commands(script) else {
                return true;
            };
            // Flattened commands do not retain control flow or subshell scope.
            // A directory change anywhere in the script invalidates its base.
            let working_directory =
                working_directory.filter(|_| !commands.iter().any(|nested| changes_shell_directory(nested)));
            return commands
                .iter()
                .any(|nested| recursive_rm_targets_are_sensitive_in_context(nested, working_directory, mount_points));
        }
    }

    if executable != "rm" {
        return false;
    }

    let Some(arguments) = command.get(1..) else {
        return false;
    };
    let parsed = parse_rm_arguments(arguments);
    if !parsed.recursive {
        return false;
    }
    if parsed.targets.is_empty() {
        return false;
    }

    let normalized_mount_points = mount_points
        .iter()
        .map(|mount_point| {
            vtcode_commons::paths::canonicalize(mount_point)
                .unwrap_or_else(|_| vtcode_commons::paths::normalize_path(mount_point))
        })
        .collect::<Vec<_>>();

    parsed.targets.into_iter().any(|target| {
        let Some(target) = resolve_target_path(target, working_directory) else {
            // A shell expansion or inaccessible working directory can hide a
            // mounted target, so unresolved recursive targets require review.
            return true;
        };

        is_known_mount_layout_target(&target)
            || normalized_mount_points
                .iter()
                .any(|mount_point| is_mount_root_or_top_level_target(&target, mount_point))
    })
}

fn parse_rm_shell_commands(script: &str) -> Option<Vec<Vec<String>>> {
    // AST extraction preserves source quotes. Decode each word once at this
    // boundary so nested shell scripts and quoted options retain argv semantics.
    super::shell_parser::parse_shell_commands_tree_sitter(script)
        .ok()?
        .into_iter()
        .map(|command| {
            command
                .into_iter()
                .map(|word| {
                    // shell_words does not interpret Bash ANSI-C quoting.
                    if word.contains("$'") {
                        return None;
                    }
                    let mut words = shell_words::split(&word).ok()?;
                    if words.len() == 1 { words.pop() } else { None }
                })
                .collect()
        })
        .collect()
}

fn changes_shell_directory(command: &[String]) -> bool {
    let Some(mut command) = unwrap_command_prefix(command) else {
        return false;
    };
    while let Some((executable, arguments)) = command.split_first() {
        match executable.as_str() {
            "cd" | "pushd" | "popd" => return true,
            "builtin" | "command" => {
                command = arguments;
                while let Some((argument, arguments)) = command.split_first() {
                    if !argument.starts_with('-') {
                        break;
                    }
                    command = arguments;
                }
            }
            _ => return false,
        }
    }
    false
}

fn resolve_target_path(target: &str, working_directory: Option<&Path>) -> Option<PathBuf> {
    if target
        .chars()
        .any(|character| matches!(character, '$' | '`' | '*' | '?' | '[' | ']' | '{' | '}' | '~'))
    {
        return None;
    }

    let target_path = Path::new(target);
    let candidate = if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        let working_directory = working_directory?;
        let working_directory = if working_directory.is_absolute() {
            working_directory.to_path_buf()
        } else {
            std::env::current_dir().ok()?.join(working_directory)
        };
        working_directory.join(target_path)
    };

    canonicalize_allowing_missing(&candidate)
}

fn canonicalize_allowing_missing(path: &Path) -> Option<PathBuf> {
    let mut ancestor = path;
    let mut missing_components = Vec::new();

    loop {
        match vtcode_commons::paths::canonicalize(ancestor) {
            Ok(mut resolved) => {
                for component in missing_components.iter().rev() {
                    resolved.push(component);
                }
                return Some(vtcode_commons::paths::normalize_path(&resolved));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing_components.push(ancestor.file_name()?.to_os_string());
                ancestor = ancestor.parent()?;
            }
            Err(_) => return None,
        }
    }
}

fn is_known_mount_layout_target(target: &Path) -> bool {
    path_is_within_depth(target, Path::new("/Volumes"), 2)
        || path_is_within_depth(target, Path::new("/media"), 3)
        || is_wsl_drive_layout_target(target)
}

fn path_is_within_depth(target: &Path, root: &Path, max_relative_depth: usize) -> bool {
    target
        .strip_prefix(root)
        .is_ok_and(|relative| relative.components().count() <= max_relative_depth)
}

fn is_wsl_drive_layout_target(target: &Path) -> bool {
    let root = Path::new("/mnt");
    if target == root {
        return true;
    }

    let Ok(relative) = target.strip_prefix(root) else {
        return false;
    };
    let mut components = relative.components();
    let Some(std::path::Component::Normal(drive)) = components.next() else {
        return false;
    };
    let Some(drive) = drive.to_str() else {
        return false;
    };
    drive.len() == 1 && drive.bytes().next().is_some_and(|byte| byte.is_ascii_alphabetic()) && components.count() <= 1
}

fn is_mount_root_or_top_level_target(target: &Path, mount_point: &Path) -> bool {
    target == mount_point
        || target.parent() == Some(mount_point)
        // Removing an ancestor recursively would traverse into the mount.
        || mount_point.starts_with(target)
}

#[cfg(target_os = "linux")]
fn mounted_filesystem_mount_points() -> Option<Vec<PathBuf>> {
    parse_linux_mount_info(&fs::read_to_string("/proc/self/mountinfo").ok()?)
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn mounted_filesystem_mount_points() -> Option<Vec<PathBuf>> {
    let output = Command::new("/sbin/mount").output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_bsd_mount_output(&String::from_utf8(output.stdout).ok()?)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
fn mounted_filesystem_mount_points() -> Option<Vec<PathBuf>> {
    None
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_mount_info(contents: &str) -> Option<Vec<PathBuf>> {
    parse_mount_points(contents.lines().map(|line| line.split_whitespace().nth(4)))
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    test
))]
fn parse_bsd_mount_output(contents: &str) -> Option<Vec<PathBuf>> {
    parse_mount_points(contents.lines().map(|line| {
        let (_, mount_description) = line.split_once(" on ")?;
        let (mount_point, _) = mount_description.rsplit_once(" (")?;
        Some(mount_point)
    }))
}

#[cfg(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    test
))]
fn parse_mount_points<'a>(fields: impl Iterator<Item = Option<&'a str>>) -> Option<Vec<PathBuf>> {
    let mount_points: Option<Vec<PathBuf>> = fields
        .map(|field| {
            let decoded = decode_mount_path(field?)?;
            let path = PathBuf::from(decoded);
            path.is_absolute().then_some(path)
        })
        .collect();
    let mount_points = mount_points?;
    (!mount_points.is_empty()).then_some(mount_points)
}

#[cfg(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    test
))]
fn decode_mount_path(encoded: &str) -> Option<String> {
    let decoded = encoded
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\");
    if decoded.is_empty() {
        return None;
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(args: &[&str]) -> Vec<String> {
        args.iter().map(|argument| (*argument).to_owned()).collect()
    }

    #[test]
    fn rm_parser_reads_recursive_options_and_end_of_options_targets() {
        for args in [
            command(&["-r", "/mnt/c"]),
            command(&["-rf", "/mnt/c"]),
            command(&["-fiR", "/mnt/c"]),
            command(&["--recursive", "/mnt/c"]),
        ] {
            let parsed = parse_rm_arguments(&args);
            assert!(parsed.recursive, "expected recursive option: {args:?}");
            assert_eq!(parsed.targets, ["/mnt/c"]);
        }

        let args = command(&["--", "-r", "/mnt/c"]);
        let after_separator = parse_rm_arguments(&args);
        assert!(!after_separator.recursive);
        assert_eq!(after_separator.targets, ["-r", "/mnt/c"]);

        let args = command(&["-fiR", "--", "-target", "/mnt/c"]);
        let parsed = parse_rm_arguments(&args);
        assert!(parsed.recursive);
        assert_eq!(parsed.targets, ["-target", "/mnt/c"]);
    }

    #[test]
    fn recursive_rm_prompts_for_mount_roots_and_immediate_children() {
        let mounts = [PathBuf::from("/"), PathBuf::from("/custom/archive")];
        for target in [
            "/Applications",
            "/Volumes",
            "/Volumes/Backup",
            "/Volumes/Backup/Photos",
            "/media",
            "/media/user/USB/Photos",
            "/mnt/c",
            "/mnt/c/Users",
            "/custom/archive",
            "/custom/archive/Photos",
        ] {
            assert!(
                recursive_rm_targets_are_sensitive(
                    &command(&["rm", "--recursive", target]),
                    Path::new("/tmp"),
                    &mounts
                ),
                "expected prompt for {target}"
            );
        }

        for invocation in [
            command(&["rm", "-r", "/Volumes"]),
            command(&["rm", "-rf", "/Volumes/Backup"]),
            command(&["rm", "-fiR", "/media/user/USB/Photos"]),
            command(&["rm", "--recursive", "/mnt/c/Users"]),
            command(&["rm", "-r", "--", "/custom/archive/Photos"]),
            command(&["bash", "-lc", "rm --recursive /custom/archive/Photos"]),
        ] {
            assert!(
                recursive_rm_targets_are_sensitive(&invocation, Path::new("/tmp"), &mounts),
                "expected prompt for {invocation:?}"
            );
        }

        for target in [
            "/Volumes/Backup/Photos/2026",
            "/custom/archive/Photos/2026",
            "/tmp/project/target",
        ] {
            assert!(
                !recursive_rm_targets_are_sensitive(&command(&["rm", "-r", target]), Path::new("/tmp"), &mounts),
                "ordinary nested target should not be newly classified: {target}"
            );
        }
    }

    #[test]
    fn recursive_rm_resolves_relative_targets_from_working_directory() {
        let temp_dir = tempfile::tempdir().expect("temporary directory");
        let mount_point = temp_dir.path().join("drive");
        fs::create_dir_all(mount_point.join("Projects")).expect("create mount fixture");
        let mounts = [mount_point.clone()];

        assert!(recursive_rm_targets_are_sensitive(&command(&["rm", "-rf", "Projects"]), &mount_point, &mounts));
        assert!(recursive_rm_targets_are_sensitive(&command(&["rm", "-r", "--", "Projects"]), &mount_point, &mounts));
        assert!(!recursive_rm_targets_are_sensitive(
            &command(&["rm", "--", "-r", "Projects"]),
            &mount_point,
            &mounts
        ));
    }

    #[test]
    fn recursive_rm_after_shell_directory_changes_requires_approval() {
        let project = tempfile::tempdir().expect("project directory");
        let mounts = [PathBuf::from("/Volumes/Backup")];

        for script in [
            "cd /Volumes/Backup && rm --recursive Photos",
            "cd /Volumes/Backup && rm --recursive Photos &&",
            "'cd' /Volumes/Backup && 'rm' '--recursive' 'Photos'",
            "$'cd' /Volumes/Backup && rm --recursive Photos",
            "builtin cd /Volumes/Backup; rm -r Photos",
            "command -- cd /Volumes/Backup && rm -R Photos",
            "pushd /Volumes/Backup && rm --recursive Photos",
            "popd; rm --recursive Photos",
            "if cd /Volumes/Backup; then rm --recursive Photos; fi",
            "(cd /Volumes/Backup && rm --recursive Photos)",
            "cd /Volumes/Backup && sh -c 'rm --recursive Photos'",
            "sh -c 'cd /Volumes/Backup && rm --recursive Photos'",
            // Flattening cannot establish directory scope or ordering safely.
            "rm --recursive Photos; cd /Volumes/Backup",
        ] {
            assert!(
                recursive_rm_targets_require_approval_with_mount_points(
                    &command(&["bash", "-lc", script]),
                    project.path(),
                    Some(&mounts),
                ),
                "relative recursive target needs approval with uncertain directory: {script}"
            );
        }

        let absolute_cleanup =
            format!("cd /Volumes/Backup && rm --recursive '{}'", project.path().join("build/cache").display());
        for script in [
            "rm --recursive Photos",
            "echo cd /Volumes/Backup && rm --recursive Photos",
            "cd /Volumes/Backup && rm -- Photos",
            absolute_cleanup.as_str(),
        ] {
            assert!(
                !recursive_rm_targets_require_approval_with_mount_points(
                    &command(&["bash", "-lc", script]),
                    project.path(),
                    Some(&mounts),
                ),
                "known ordinary targets or nonrecursive deletes should retain their classification: {script}"
            );
        }
    }

    #[test]
    fn recursive_rm_with_unresolved_targets_fails_closed() {
        let command = command(&["rm", "--recursive", "$TARGET"]);
        assert!(recursive_rm_targets_are_sensitive(&command, Path::new("/tmp"), &[]));
        assert!(recursive_rm_targets_require_approval_with_mount_points(&command, Path::new("/tmp"), Some(&[])));
    }

    #[test]
    fn ordinary_recursive_project_cleanup_stays_unchanged_with_mount_data() {
        let temp_dir = tempfile::tempdir().expect("temporary directory");
        let target = temp_dir.path().join("build/cache");
        fs::create_dir_all(&target).expect("create project fixture");
        let command = command(&["rm", "--recursive", "build/cache"]);
        let mount_points = [PathBuf::from("/"), PathBuf::from("/custom/archive")];

        assert!(!recursive_rm_targets_require_approval_with_mount_points(
            &command,
            temp_dir.path(),
            Some(&mount_points)
        ));
    }

    #[test]
    fn recursive_rm_requires_approval_when_mount_data_is_unavailable() {
        let command = command(&["rm", "--recursive", "build/cache"]);
        assert!(recursive_rm_targets_require_approval_with_mount_points(&command, Path::new("/tmp/project"), None));
    }

    #[test]
    fn mount_tables_decode_paths_and_reject_incomplete_data() {
        let linux =
            parse_linux_mount_info("36 25 0:32 / /mnt/My\\040Drive rw - ext4 /dev/sda1 rw\n").expect("valid mountinfo");
        assert_eq!(linux, [PathBuf::from("/mnt/My Drive")]);
        assert!(parse_bsd_mount_output("/dev/disk3 on /Volumes/My\\040Drive (apfs, local)\n").is_some());
        assert!(parse_linux_mount_info("malformed\n").is_none());
        assert!(parse_bsd_mount_output("malformed\n").is_none());
    }
}
