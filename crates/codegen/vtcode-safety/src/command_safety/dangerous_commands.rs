#![expect(
    clippy::indexing_slicing,
    reason = "Dangerous-command matching uses validated token lengths and fixed ASCII option prefixes."
)]

//! Detection of dangerous commands that should never be executed.
//!
//! This module implements hardcoded detection for commands that are inherently
//! destructive or dangerous, regardless of their options.
//!
//! Examples:
//! - `rm -rf /` (destructive)
//! - `git reset --hard` (destructive)
//! - `dd if=/dev/zero of=/dev/sda` (very destructive)
//! - `sudo rm` (privilege escalation + destruction)

/// Checks if a command appears dangerous to execute.
/// Returns true if the command should be blocked before execution.
pub fn command_might_be_dangerous(command: &[String]) -> bool {
    let Some(command) = unwrap_command_prefix(command) else {
        return !command.is_empty();
    };
    let Some(executable) = command.first() else {
        return false;
    };
    if executable_is_dynamic(executable) {
        return true;
    }

    // PowerShell's encoded-command form hides the script from every
    // platform-neutral parser. Treat it as dangerous before policy or shell
    // evaluation can classify the base64 payload as an ordinary argument.
    if is_encoded_powershell_invocation(command) {
        return true;
    }

    #[cfg(windows)]
    {
        if crate::command_safety::windows::is_dangerous_command_windows(command) {
            return true;
        }
    }

    if is_dangerous_to_call_with_exec(command) {
        return true;
    }

    // Support bash -lc "..." parsing for chained commands
    // If the command is bash -c "..." or similar, parse the script and check each command
    if command.len() >= 3
        && matches!(extract_command_name(&command[0]), "bash" | "sh" | "zsh")
        && (command[1] == "-c" || command[1] == "-lc" || command[1] == "-ilc")
    {
        let script = &command[2];
        if let Ok(sub_commands) = crate::command_safety::shell_parser::parse_shell_commands(script) {
            for sub_cmd in sub_commands {
                if command_might_be_dangerous(&sub_cmd) {
                    return true;
                }
            }
        } else {
            return true;
        }
    }

    false
}

/// Returns whether the command crosses an inline-code boundary that must be
/// admitted by an enforceable sandbox or explicit human approval.
///
/// This is deliberately separate from [`command_might_be_dangerous`]: inline
/// interpreter programs are not forbidden outright, but their source text can
/// perform arbitrary effects that argv-level command classification cannot
/// prove safe.
pub fn command_requires_approval(command: &[String]) -> bool {
    let Some(command) = unwrap_command_prefix(command) else {
        return false;
    };
    if is_inline_code_execution(command) {
        return true;
    }

    if command.len() >= 3
        && matches!(extract_command_name(&command[0]), "bash" | "sh" | "zsh")
        && matches!(command[1].as_str(), "-c" | "-lc" | "-ilc")
        && let Ok(commands) = crate::command_safety::shell_parser::parse_shell_commands(&command[2])
    {
        return commands.iter().any(|nested| command_requires_approval(nested));
    }

    false
}

fn executable_is_dynamic(executable: &str) -> bool {
    executable
        .chars()
        .any(|character| matches!(character, '$' | '`' | '*' | '?' | '[' | ']' | '{' | '}'))
}

fn is_environment_assignment(argument: &str) -> bool {
    let Some((name, _value)) = argument.split_once('=') else {
        return false;
    };
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(super) fn unwrap_command_prefix(mut command: &[String]) -> Option<&[String]> {
    loop {
        while command.first().is_some_and(|argument| is_environment_assignment(argument)) {
            command = &command[1..];
        }
        let executable = command.first()?;
        match extract_command_name(executable) {
            "env" => {
                command = &command[1..];
                while let Some(argument) = command.first().map(String::as_str) {
                    if is_environment_assignment(argument) || matches!(argument, "-i" | "--ignore-environment") {
                        command = &command[1..];
                    } else if matches!(argument, "-u" | "--unset") {
                        command = command.get(2..)?;
                    } else if argument.starts_with("--unset=") {
                        command = &command[1..];
                    } else if argument == "--" {
                        command = &command[1..];
                        break;
                    } else if argument.starts_with('-') {
                        return None;
                    } else {
                        break;
                    }
                }
            }
            "sudo" => {
                command = &command[1..];
                while let Some(argument) = command.first().map(String::as_str) {
                    if argument == "--" {
                        command = &command[1..];
                        break;
                    }
                    if matches!(argument, "-u" | "--user" | "-g" | "--group" | "-h" | "--host" | "-C" | "--chdir") {
                        command = command.get(2..)?;
                    } else if matches!(argument, "-E" | "-H" | "-n" | "-S" | "-k" | "-K" | "-b")
                        || argument.starts_with("--user=")
                        || argument.starts_with("--group=")
                        || argument.starts_with("--host=")
                        || argument.starts_with("--chdir=")
                    {
                        command = &command[1..];
                    } else if argument.starts_with('-') {
                        return None;
                    } else {
                        break;
                    }
                }
            }
            _ => return Some(command),
        }
    }
}

fn is_inline_code_execution(command: &[String]) -> bool {
    let Some(executable) = command.first().map(|value| extract_command_name(value).to_ascii_lowercase()) else {
        return false;
    };
    let arguments = &command[1..];
    match executable.as_str() {
        "python" | "python3" | "python.exe" | "python3.exe" => arguments.iter().any(|argument| argument == "-c"),
        "node" | "node.exe" | "ruby" | "ruby.exe" | "perl" | "perl.exe" | "osascript" => {
            arguments.iter().any(|argument| argument == "-e")
        }
        "php" | "php.exe" => arguments.iter().any(|argument| argument == "-r"),
        "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe" => arguments.iter().any(|argument| {
            ["-command", "-c", "-encodedcommand", "-encoded", "-enc", "-e"]
                .iter()
                .any(|option| argument.eq_ignore_ascii_case(option))
        }),
        _ => false,
    }
}

fn is_encoded_powershell_invocation(command: &[String]) -> bool {
    let Some(executable) = command.first() else {
        return false;
    };
    let executable = extract_command_name(executable).to_ascii_lowercase();
    if !matches!(executable.as_str(), "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe") {
        return false;
    }

    command.iter().skip(1).any(|argument| {
        ["-encodedcommand", "-encoded", "-enc", "-e"]
            .iter()
            .any(|option| argument.eq_ignore_ascii_case(option))
    })
}

/// Git global options that take a value (skip these and their values when finding subcommand)
fn is_git_global_option_with_value(arg: &str) -> bool {
    matches!(
        arg,
        "-C" | "-c" | "--config-env" | "--exec-path" | "--git-dir" | "--namespace" | "--super-prefix" | "--work-tree"
    )
}

/// Git global options with inline values (e.g., --git-dir=/path)
fn is_git_global_option_with_inline_value(arg: &str) -> bool {
    matches!(
        arg,
        s if s.starts_with("--config-env=")
            || s.starts_with("--exec-path=")
            || s.starts_with("--git-dir=")
            || s.starts_with("--namespace=")
        || s.starts_with("--super-prefix=")
        || s.starts_with("--work-tree=")
    ) || ((arg.starts_with("-C") || arg.starts_with("-c")) && arg.len() > 2)
}

/// Returns whether a git global option can redirect repository, config, or
/// helper lookup and therefore must not be treated as an inspection flag.
pub fn git_global_option_requires_prompt(arg: &str) -> bool {
    matches!(
        arg,
        "-C" | "-c" | "--config-env" | "--exec-path" | "--git-dir" | "--namespace" | "--super-prefix" | "--work-tree"
    ) || matches!(
        arg,
        s if (s.starts_with("-C") && s.len() > 2)
            || (s.starts_with("-c") && s.len() > 2)
            || s.starts_with("--config-env=")
            || s.starts_with("--exec-path=")
            || s.starts_with("--git-dir=")
            || s.starts_with("--namespace=")
            || s.starts_with("--super-prefix=")
            || s.starts_with("--work-tree=")
    )
}

/// Find the first matching git subcommand, skipping known global options that
/// may appear before it (e.g., `-C`, `-c`, `--git-dir`).
///
/// Shared with `is_safe_command` to avoid git-global-option bypasses.
pub(crate) fn find_git_subcommand<'a>(command: &'a [String], subcommands: &[&str]) -> Option<(usize, &'a str)> {
    let cmd0 = command.first().map(String::as_str)?;
    if !cmd0.ends_with("git") {
        return None;
    }

    let mut skip_next = false;
    for (idx, arg) in command.iter().enumerate().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }

        let arg = arg.as_str();

        if is_git_global_option_with_inline_value(arg) {
            continue;
        }

        if is_git_global_option_with_value(arg) {
            skip_next = true;
            continue;
        }

        if arg == "--" || arg.starts_with('-') {
            continue;
        }

        if subcommands.contains(&arg) {
            return Some((idx, arg));
        }

        // In git, the first non-option token is the subcommand. If it isn't
        // one of the subcommands we're looking for, we must stop scanning to
        // avoid misclassifying later positional args (e.g., branch names).
        return None;
    }

    None
}

/// Check if a short flag group contains a specific character (e.g., -fdx contains 'f')
fn short_flag_group_contains(arg: &str, target: char) -> bool {
    arg.starts_with('-') && !arg.starts_with("--") && arg.chars().skip(1).any(|c| c == target)
}

/// Check if git push command is dangerous (force, delete, or dangerous refspec)
fn git_push_is_dangerous(push_args: &[String]) -> bool {
    push_args.iter().map(String::as_str).any(|arg| {
        matches!(arg, "--force" | "--force-with-lease" | "--force-if-includes" | "--delete" | "-f" | "-d")
            || arg.starts_with("--force-with-lease=")
            || arg.starts_with("--force-if-includes=")
            || arg.starts_with("--delete=")
            || short_flag_group_contains(arg, 'f')
            || short_flag_group_contains(arg, 'd')
            || git_push_refspec_is_dangerous(arg)
    })
}

/// Check if a refspec is dangerous (+refspec forces updates, :refspec deletes)
fn git_push_refspec_is_dangerous(arg: &str) -> bool {
    // `+<refspec>` forces updates and `:<dst>` deletes remote refs.
    (arg.starts_with('+') || arg.starts_with(':')) && arg.len() > 1
}

/// Check if git clean command uses force flag
fn git_clean_is_force(clean_args: &[String]) -> bool {
    clean_args.iter().map(String::as_str).any(|arg| {
        matches!(arg, "--force" | "-f") || arg.starts_with("--force=") || short_flag_group_contains(arg, 'f')
    })
}

/// Git subcommands whose destructive modes are hard-blocked at preflight.
/// Recoverable invocations of these subcommands (`git reset --soft`,
/// `git rm --cached`, `git branch -d`) pass preflight and proceed through
/// normal policy/approval routing, matching `exec_policy`'s `validate_git_reset`.
const GIT_GUARDED_SUBCOMMANDS: &[&str] = &["reset", "rm", "branch", "push", "clean"];

/// Only the hard reset modes lose uncommitted changes. `--soft`/`--mixed`
/// (and a bare reset) move HEAD or the index, which the reflog restores.
fn git_reset_is_destructive(reset_args: &[String]) -> bool {
    reset_args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--hard" | "--merge" | "--keep"))
}

/// `git rm --cached` only unstages paths (the working tree is untouched);
/// every other `git rm` deletes working-tree files.
fn git_rm_is_destructive(rm_args: &[String]) -> bool {
    !rm_args.iter().any(|arg| arg == "--cached")
}

/// `git branch -d`/`--delete` alone refuses unmerged branches; `-D` (or
/// `--delete` combined with `-f`/`--force`, including stacked short flags
/// like `-dD`) overrides that guard. Delete plus force is the destructive
/// combination.
fn git_branch_is_force_delete(branch_args: &[String]) -> bool {
    let mut deletes = false;
    let mut forces = false;
    for arg in branch_args {
        match arg.as_str() {
            "-D" => {
                deletes = true;
                forces = true;
            }
            "-d" | "--delete" => deletes = true,
            "-f" | "--force" => forces = true,
            _ => {
                if arg.starts_with("--delete=") {
                    deletes = true;
                }
                if arg.starts_with("--force=") {
                    forces = true;
                }
                if short_flag_group_contains(arg, 'd') {
                    deletes = true;
                }
                if short_flag_group_contains(arg, 'D') {
                    deletes = true;
                    forces = true;
                }
                if short_flag_group_contains(arg, 'f') {
                    forces = true;
                }
            }
        }
    }
    deletes && forces
}

/// Single home for the guarded git subcommand decision. Returns the matched
/// destructive pattern (for actionable preflight messages), or `None` when
/// the invocation is recoverable and must proceed through normal policy and
/// approval routing instead of dying at preflight.
fn classify_git_subcommand(subcommand: &str, args: &[String]) -> Option<&'static str> {
    // `--` ends option parsing: for the option-only subcommands every later
    // token is a path or ref name, never a flag (`git rm -- --cached f`
    // deletes working-tree files). Classify only the pre-`--` option
    // arguments there; destructive flags before `--` still classify, and a
    // post-`--` token can only turn a block into a pass when it is genuinely
    // a name. `push` is excluded: its dangerous payloads are refspecs, which
    // legitimately occupy the post-`--` positional slot
    // (`git push origin -- :refs/heads/x` deletes a remote branch).
    let scan_args = if subcommand == "push" {
        args
    } else {
        match args.iter().position(|arg| arg == "--") {
            Some(end) => &args[..end],
            None => args,
        }
    };
    match subcommand {
        "reset" if git_reset_is_destructive(scan_args) => {
            Some("git reset --hard/--merge/--keep discards uncommitted changes; use `git stash` or `git reset --soft`")
        }
        "rm" if git_rm_is_destructive(scan_args) => {
            Some("git rm deletes working-tree files; unstage with `git rm --cached` instead")
        }
        "branch" if git_branch_is_force_delete(scan_args) => {
            Some("git branch -D/--delete --force skips the unmerged-branch guard; use `-d` for merged branches")
        }
        "push" if git_push_is_dangerous(scan_args) => Some("git push force-updates or deletes remote refs"),
        "clean" if git_clean_is_force(scan_args) => Some("git clean --force deletes untracked files"),
        _ => None,
    }
}

/// Reason a command tripped [`command_might_be_dangerous`], used for
/// actionable preflight messages. Mirrors the git arm of
/// `is_dangerous_to_call_with_exec` including env/sudo prefix unwrapping;
/// non-git patterns keep the generic rejection text.
pub fn dangerous_command_reason(command: &[String]) -> Option<&'static str> {
    let command = unwrap_command_prefix(command)?;
    let cmd0 = command.first().map(String::as_str);
    let (idx, subcommand) = if extract_command_name(cmd0.unwrap_or("")) == "git" {
        find_git_subcommand(command, GIT_GUARDED_SUBCOMMANDS)?
    } else {
        // Without the git executable in front, `rm` is ambiguous with the
        // plain Unix command — `rm -f`/`sudo rm -rf` must not receive the
        // `git rm --cached` remedy.
        let (idx, subcommand) = find_git_subcommand_from_args(command, GIT_GUARDED_SUBCOMMANDS)?;
        if subcommand == "rm" {
            return None;
        }
        (idx, subcommand)
    };
    classify_git_subcommand(subcommand, &command[idx + 1..])
}

/// Check if a command is a dangerous git subcommand (without the "git" prefix)
/// This handles commands parsed from shell scripts where the binary name may be omitted
fn is_dangerous_git_subcommand(command: &[String]) -> bool {
    find_git_subcommand_from_args(command, GIT_GUARDED_SUBCOMMANDS)
        .and_then(|(idx, subcommand)| classify_git_subcommand(subcommand, &command[idx + 1..]))
        .is_some()
}

/// Find git subcommand from a list of args (without the "git" binary name)
fn find_git_subcommand_from_args<'a>(args: &'a [String], subcommands: &[&str]) -> Option<(usize, &'a str)> {
    let mut skip_next = false;
    for (idx, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }

        let arg = arg.as_str();

        if is_git_global_option_with_inline_value(arg) {
            continue;
        }

        if is_git_global_option_with_value(arg) {
            skip_next = true;
            continue;
        }

        if arg == "--" || arg.starts_with('-') {
            continue;
        }

        if subcommands.contains(&arg) {
            return Some((idx, arg));
        }

        // First non-option token that isn't a subcommand we're looking for
        return None;
    }

    None
}

/// Core dangerous command detection for Unix/Linux/macOS
fn is_dangerous_to_call_with_exec(command: &[String]) -> bool {
    if command.is_empty() {
        return false;
    }

    let cmd0 = command.first().map(String::as_str);
    let base_cmd = extract_command_name(cmd0.unwrap_or(""));

    match base_cmd {
        // ──── Git ────
        "git" => {
            let Some((subcommand_idx, subcommand)) = find_git_subcommand(command, GIT_GUARDED_SUBCOMMANDS) else {
                return false;
            };

            classify_git_subcommand(subcommand, &command[subcommand_idx + 1..]).is_some()
        }

        // ──── Rm ────
        "rm" => matches!(command.get(1).map(String::as_str), Some("-f" | "-rf" | "-fr" | "-r")),

        // ──── Destructive system commands ────
        _ if base_cmd == "mkfs" || base_cmd.starts_with("mkfs.") => true,
        "dd" | "shutdown" | "reboot" | "init" => true,

        // ──── Fork bomb ────
        _ if base_cmd.ends_with(':') && command.len() >= 2 => command[1] == "(){:|:&};:",

        // ──── Sudo: check the wrapped command ────
        "sudo" => {
            if command.len() > 1 {
                is_dangerous_to_call_with_exec(&command[1..])
            } else {
                false
            }
        }

        // ──── Git subcommands without "git" prefix (from shell parsing) ────
        _ => is_dangerous_git_subcommand(command),
    }
}

/// Extract base command name from full path
fn extract_command_name(cmd: &str) -> &str {
    std::path::Path::new(cmd)
        .file_name()
        .and_then(|osstr| osstr.to_str())
        .unwrap_or(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec_str(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn powershell_encoded_options_remain_hard_denials_in_any_ascii_case() {
        for executable in ["powershell", "PowerShell.exe", "pwsh", "PWSH.EXE", "/usr/bin/pwsh"] {
            for option in ["-encodedcommand", "-encoded", "-enc", "-e"] {
                for variant in [option.to_string(), option.to_ascii_uppercase()] {
                    let command = vec_str(&[executable, "-NoProfile", &variant, "ZQ=="]);
                    assert!(command_might_be_dangerous(&command), "hard denial: {command:?}");
                    assert!(command_requires_approval(&command), "approval classification: {command:?}");
                }
            }
        }
        let wrapped = vec_str(&["env", "MODE=test", "pwsh", "-EnCoDeDcOmMaNd", "ZQ=="]);
        assert!(command_might_be_dangerous(&wrapped));
        assert!(command_requires_approval(&wrapped));
    }

    #[test]
    fn powershell_inline_options_require_approval_without_hard_denial() {
        for executable in ["powershell", "powershell.exe", "pwsh", "pwsh.exe"] {
            for option in ["-command", "-c", "-Command", "-COMMAND", "-C"] {
                let command = vec_str(&[executable, "-NoProfile", option, "Write-Output ok"]);
                assert!(!command_might_be_dangerous(&command), "inline code: {command:?}");
                assert!(command_requires_approval(&command), "approval: {command:?}");
            }
        }
    }

    #[test]
    fn powershell_option_near_misses_and_non_powershell_programs_stay_distinct() {
        for option in [
            "-commands",
            "-encodedcommandx",
            "--enc",
            "enc",
            "-en",
            "-e=ZQ==",
            "-command ",
            " -command",
            "-commánd",
            "-enç",
        ] {
            let command = vec_str(&["pwsh", option, "example.ps1"]);
            assert!(!command_might_be_dangerous(&command), "near miss: {command:?}");
            assert!(!command_requires_approval(&command), "near miss: {command:?}");
        }
        for command in [
            vec_str(&["pwsh", "-File", "example.ps1"]),
            vec_str(&["echo", "-ENCODEDCOMMAND", "ZQ=="]),
        ] {
            assert!(!command_might_be_dangerous(&command), "ordinary arguments: {command:?}");
            assert!(!command_requires_approval(&command), "ordinary arguments: {command:?}");
        }
    }

    #[test]
    fn git_reset_recoverable_modes_pass_preflight() {
        // Bare reset and --soft/--mixed only move HEAD or the index; the
        // reflog restores them, matching exec_policy's validate_git_reset.
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "reset"])));
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "reset", "--soft", "HEAD~1"])));
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "reset", "--mixed", "HEAD~1"])));
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "reset", "HEAD~1", "--", "file.txt"])));
    }

    #[test]
    fn git_reset_destructive_modes_still_preflight_blocked() {
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "reset", "--hard"])));
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "reset", "--merge"])));
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "reset", "--keep"])));
        // Destructive mode hidden behind global options and sudo wrappers.
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "-C", "sub", "reset", "--hard"])));
        assert!(command_might_be_dangerous(&vec_str(&["sudo", "git", "reset", "--hard"])));
        assert!(
            command_might_be_dangerous(&vec_str(&["FOO=bar", "env", "git", "reset", "--hard"])),
            "env-prefixed destructive reset must stay blocked"
        );
    }

    #[test]
    fn git_rm_index_only_passes_preflight_and_working_tree_delete_stays_blocked() {
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "rm", "file.txt"])));
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "rm", "-rf", "dir"])));
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "rm", "--cached", "file.txt"])));
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "rm", "-r", "--cached", "dir"])));
        assert!(
            is_dangerous_to_call_with_exec(&vec_str(&["sudo", "git", "rm", "file.txt"])),
            "sudo-wrapped working-tree delete must stay blocked"
        );
    }

    #[test]
    fn git_branch_force_delete_is_blocked_but_merged_delete_passes() {
        // -d/--delete refuse unmerged branches; only the force forms are blocked.
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "branch", "-d", "feature"])));
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "branch", "--delete", "feature"])));
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "branch", "-D", "feature"])));
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "branch", "--delete", "--force", "feature"])));
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "branch", "-d", "-f", "feature"])));
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "branch", "-df", "feature"])));
    }

    #[test]
    fn end_of_options_separator_cannot_hide_working_tree_git_rm() {
        // After `--`, git treats `--cached` as a PATH, not the index-only
        // flag: `git rm --ignore-unmatch -- --cached f` deletes working-tree
        // files while carrying a literal `--cached` argument.
        assert!(
            is_dangerous_to_call_with_exec(&vec_str(&["git", "rm", "--", "--cached", "file.txt"])),
            "`--cached` after `--` is a path; the invocation deletes working-tree files"
        );
        assert!(
            is_dangerous_to_call_with_exec(&vec_str(&["git", "rm", "--ignore-unmatch", "--", "--cached", "file.txt"])),
            "--ignore-unmatch must not let a missing `--cached` path mask the deletion"
        );
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["sudo", "git", "rm", "--", "--cached", "file.txt"])));
        // The flag itself before `--` keeps the index-only pass.
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "rm", "--cached", "--", "file.txt"])));
        // The rejection must keep the pattern-specific remedy.
        let reason = dangerous_command_reason(&vec_str(&["git", "rm", "--", "--cached", "file.txt"]))
            .expect("end-of-options git rm carries a reason");
        assert!(reason.contains("git rm"), "reason should name the pattern: {reason}");
    }

    #[test]
    fn end_of_options_separator_stops_flag_classification_for_other_guarded_subcommands() {
        // Post-`--` tokens are paths/refs for the option-only subcommands, so
        // they can never carry flag semantics.
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "reset", "--", "--hard"])));
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "branch", "--", "-D", "feature"])));
        assert!(!is_dangerous_to_call_with_exec(&vec_str(&["git", "clean", "--", "-fd"])));
        // Flags before `--` still classify.
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "reset", "--hard", "--", "file.txt"])));
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "branch", "-D", "--", "feature"])));
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "clean", "-fd", "--", "dir"])));
        // `push` scans all args: its dangerous payloads are refspecs, which
        // legitimately occupy the post-`--` positional slot.
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "push", "origin", "--", "--force"])));
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "push", "origin", "--", ":refs/heads/main"])));
        assert!(is_dangerous_to_call_with_exec(&vec_str(&["git", "push", "--force", "--", "origin", "main"])));
    }

    #[test]
    fn dangerous_command_reason_names_git_pattern_and_skips_safe_commands() {
        let destructive = vec_str(&["git", "reset", "--hard"]);
        let reason = dangerous_command_reason(&destructive).expect("git reset --hard carries a reason");
        assert!(reason.contains("git reset"), "reason should name the pattern: {reason}");

        let recoverable = vec_str(&["git", "reset", "--soft", "HEAD~1"]);
        assert!(dangerous_command_reason(&recoverable).is_none());

        let safe = vec_str(&["git", "status"]);
        assert!(dangerous_command_reason(&safe).is_none());

        let pushed = vec_str(&["sudo", "git", "rm", "file.txt"]);
        let sudo_reason = dangerous_command_reason(&pushed).expect("sudo-wrapped git rm carries a reason");
        assert!(sudo_reason.contains("git rm"), "sudo prefix must be unwrapped: {sudo_reason}");
    }

    #[test]
    fn dangerous_command_reason_does_not_label_plain_rm_as_git_rm() {
        // `rm -f` is dangerous via the plain-rm arm, not the git matcher, so
        // it must keep the generic rejection instead of the `git rm --cached`
        // remedy.
        for cmd in [
            vec_str(&["rm", "-f", "build.log"]),
            vec_str(&["rm", "-rf", "dir"]),
            vec_str(&["sudo", "rm", "-rf", "/tmp/x"]),
        ] {
            assert!(command_might_be_dangerous(&cmd), "plain rm must stay classified dangerous: {cmd:?}");
            assert_eq!(dangerous_command_reason(&cmd), None, "plain rm must not receive the git rm remedy: {cmd:?}");
        }
    }

    #[test]
    fn git_status_is_safe() {
        let cmd = vec!["git".to_string(), "status".to_string()];
        assert!(!is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn git_log_is_safe() {
        let cmd = vec!["git".to_string(), "log".to_string()];
        assert!(!is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn rm_f_is_dangerous() {
        let cmd = vec!["rm".to_string(), "-f".to_string(), "file.txt".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn rm_rf_is_dangerous() {
        let cmd = vec!["rm".to_string(), "-rf".to_string(), "/".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn rm_without_flags_is_safe() {
        let cmd = vec!["rm".to_string()];
        assert!(!is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn mkfs_is_dangerous() {
        let cmd = vec!["mkfs".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn mkfs_variants_are_dangerous() {
        let cmd = vec!["mkfs.ext4".to_string(), "/dev/sda1".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn dd_is_dangerous() {
        let cmd = vec!["dd".to_string(), "if=/dev/zero".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn shutdown_is_dangerous() {
        let cmd = vec!["shutdown".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn sudo_git_reset_is_dangerous() {
        let cmd = vec![
            "sudo".to_string(),
            "git".to_string(),
            "reset".to_string(),
            "--hard".to_string(),
        ];
        assert!(is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn sudo_git_status_is_safe() {
        let cmd = vec!["sudo".to_string(), "git".to_string(), "status".to_string()];
        assert!(!is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn absolute_path_git_reset_hard_is_dangerous() {
        let cmd = vec!["/usr/bin/git".to_string(), "reset".to_string(), "--hard".to_string()];
        assert!(is_dangerous_to_call_with_exec(&cmd));
        // A bare reset via an absolute path is mixed-mode and recoverable.
        let bare = vec!["/usr/bin/git".to_string(), "reset".to_string()];
        assert!(!is_dangerous_to_call_with_exec(&bare));
    }

    #[test]
    fn empty_command_is_safe() {
        let cmd: Vec<String> = vec![];
        assert!(!is_dangerous_to_call_with_exec(&cmd));
    }

    #[test]
    fn command_might_be_dangerous_detects_git_reset_hard() {
        let cmd = vec!["git".to_string(), "reset".to_string(), "--hard".to_string()];
        assert!(command_might_be_dangerous(&cmd));
        // Bare reset (mixed mode) is recoverable and passes preflight.
        let bare = vec!["git".to_string(), "reset".to_string()];
        assert!(!command_might_be_dangerous(&bare));
    }

    #[test]
    fn command_might_be_dangerous_allows_git_status() {
        let cmd = vec!["git".to_string(), "status".to_string()];
        assert!(!command_might_be_dangerous(&cmd));
    }

    #[test]
    fn wrappers_and_absolute_executables_do_not_hide_dangerous_commands() {
        assert!(command_might_be_dangerous(&vec_str(&[
            "env",
            "MODE=test",
            "sudo",
            "-u",
            "root",
            "/usr/bin/git",
            "reset",
            "--hard",
        ])));
        assert!(command_might_be_dangerous(&vec_str(&["MODE=test", "/bin/sh", "-c", "rm -rf /",])));
    }

    #[test]
    fn inline_interpreter_programs_are_code_execution_boundaries() {
        for command in [
            vec_str(&["/usr/bin/python3", "-c", "print('ok')"]),
            vec_str(&["node", "-e", "console.log('ok')"]),
            vec_str(&["ruby", "-e", "puts 'ok'"]),
            vec_str(&["perl", "-e", "print 'ok'"]),
            vec_str(&["php", "-r", "echo 'ok';"]),
            vec_str(&["osascript", "-e", "return 1"]),
            vec_str(&["pwsh", "-Command", "Write-Output ok"]),
        ] {
            assert!(!command_might_be_dangerous(&command), "inline code is not forbidden outright: {command:?}");
            assert!(command_requires_approval(&command), "inline code should require policy admission: {command:?}");
        }

        let nested = vec_str(&["bash", "-lc", "python3 -c 'print(1)'"]);
        assert!(command_requires_approval(&nested));
        assert!(!command_might_be_dangerous(&nested));
    }

    #[test]
    fn dynamic_or_unknown_wrapped_executables_fail_closed() {
        assert!(command_might_be_dangerous(&vec_str(&["$TOOL", "status"])));
        assert!(command_might_be_dangerous(&vec_str(&["env", "--unknown", "git", "status"])));
        assert!(command_might_be_dangerous(&vec_str(&["sudo", "--unknown", "git", "status"])));
    }

    // ──── Git Branch Delete Tests ────

    #[test]
    fn git_branch_delete_is_dangerous_only_when_forced() {
        // -d/--delete refuse unmerged branches, so they pass preflight.
        assert!(!command_might_be_dangerous(&vec_str(&["git", "branch", "-d", "feature",])));
        assert!(command_might_be_dangerous(&vec_str(&["git", "branch", "-D", "feature",])));
        // Test shell script parsing separately
        let script = "git branch --delete --force feature";
        if let Ok(sub_commands) = crate::command_safety::shell_parser::parse_shell_commands(script) {
            for sub_cmd in sub_commands {
                assert!(command_might_be_dangerous(&sub_cmd), "sub-command should be dangerous: {sub_cmd:?}");
            }
        }
    }

    #[test]
    fn git_branch_delete_with_stacked_short_flags_is_dangerous_only_when_forced() {
        // Plain delete groups (-dv/-vd) keep the unmerged guard; groups
        // containing D (or f) force it.
        assert!(!command_might_be_dangerous(&vec_str(&["git", "branch", "-dv", "feature",])));
        assert!(!command_might_be_dangerous(&vec_str(&["git", "branch", "-vd", "feature",])));
        assert!(command_might_be_dangerous(&vec_str(&["git", "branch", "-vD", "feature",])));
        assert!(command_might_be_dangerous(&vec_str(&["git", "branch", "-Dvv", "feature",])));
        assert!(command_might_be_dangerous(&vec_str(&["git", "branch", "-df", "feature",])));
    }

    #[test]
    fn git_branch_delete_with_global_options_is_dangerous_only_when_forced() {
        assert!(!command_might_be_dangerous(&vec_str(&["git", "-C", ".", "branch", "-d", "feature",])));
        assert!(command_might_be_dangerous(&vec_str(&["git", "-c", "color.ui=false", "branch", "-D", "feature",])));
        // Test shell script parsing separately
        let script = "git -C . branch -D feature";
        if let Ok(sub_commands) = crate::command_safety::shell_parser::parse_shell_commands(script) {
            for sub_cmd in sub_commands {
                assert!(command_might_be_dangerous(&sub_cmd), "sub-command should be dangerous: {sub_cmd:?}");
            }
        }
    }

    #[test]
    fn git_checkout_reset_is_not_dangerous() {
        // The first non-option token is "checkout", so later positional args
        // like branch names must not be treated as subcommands.
        assert!(!command_might_be_dangerous(&vec_str(&["git", "checkout", "reset",])));
    }

    // ──── Git Push Dangerous Tests ────

    #[test]
    fn git_push_force_is_dangerous() {
        assert!(command_might_be_dangerous(&vec_str(&["git", "push", "--force", "origin", "main",])));
        assert!(command_might_be_dangerous(&vec_str(&["git", "push", "-f", "origin", "main",])));
        assert!(command_might_be_dangerous(&vec_str(&[
            "git",
            "-C",
            ".",
            "push",
            "--force-with-lease",
            "origin",
            "main",
        ])));
    }

    #[test]
    fn git_push_plus_refspec_is_dangerous() {
        assert!(command_might_be_dangerous(&vec_str(&["git", "push", "origin", "+main",])));
        assert!(command_might_be_dangerous(&vec_str(
            &["git", "push", "origin", "+refs/heads/main:refs/heads/main",]
        )));
    }

    #[test]
    fn git_push_delete_flag_is_dangerous() {
        assert!(command_might_be_dangerous(&vec_str(&["git", "push", "--delete", "origin", "feature",])));
        assert!(command_might_be_dangerous(&vec_str(&["git", "push", "-d", "origin", "feature",])));
    }

    #[test]
    fn git_push_delete_refspec_is_dangerous() {
        assert!(command_might_be_dangerous(&vec_str(&["git", "push", "origin", ":feature",])));
        // Test shell script parsing separately
        let script = "git push origin :feature";
        if let Ok(sub_commands) = crate::command_safety::shell_parser::parse_shell_commands(script) {
            for sub_cmd in sub_commands {
                assert!(command_might_be_dangerous(&sub_cmd), "sub-command should be dangerous: {sub_cmd:?}");
            }
        }
    }

    #[test]
    fn git_push_without_force_is_not_dangerous() {
        assert!(!command_might_be_dangerous(&vec_str(&["git", "push", "origin", "main",])));
    }

    // ──── Git Clean Tests ────

    #[test]
    fn git_clean_force_is_dangerous_even_when_f_is_not_first_flag() {
        assert!(command_might_be_dangerous(&vec_str(&["git", "clean", "-fdx",])));
        assert!(command_might_be_dangerous(&vec_str(&["git", "clean", "-xdf",])));
        assert!(command_might_be_dangerous(&vec_str(&["git", "clean", "--force",])));
    }
}
