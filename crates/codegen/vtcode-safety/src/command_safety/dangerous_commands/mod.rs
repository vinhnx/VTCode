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
    if let Some(script) = nested_shell_script(command) {
        let script = strip_nested_script_quotes(script);
        // Command substitution is resolved by the *inner* shell, after the
        // outer single quotes have already made it inert for the top-level
        // scan (`sh -c 'echo $(touch /tmp/probe)'`). Nothing in the nested
        // text is statically known until the substitution runs, so the script
        // cannot be admitted as safe.
        if super::contains_command_substitution(script) {
            return true;
        }
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

/// Shell options that make the next argument an inline script.
const NESTED_SCRIPT_FLAGS: &[&str] = &["-c", "-lc", "-ilc"];

/// Return the inline script of a `bash`/`sh`/`zsh -c` style command.
///
/// The returned slice is raw argument text, so it may still carry the shell
/// quotes around the script; run it through [`strip_nested_script_quotes`]
/// before analyzing its contents.
pub(super) fn nested_shell_script(command: &[String]) -> Option<&str> {
    match command {
        [executable, flag, script, ..]
            if matches!(extract_command_name(executable), "bash" | "sh" | "zsh")
                && NESTED_SCRIPT_FLAGS.contains(&flag.as_str()) =>
        {
            Some(script)
        }
        _ => None,
    }
}

/// Strip one matching pair of shell quotes from a nested `-c` script.
///
/// `parse_shell_commands` preserves raw argument text, so a nested script
/// arrives as `'…'` or `"…"`. The shell removes those quotes before running the
/// script, so analyzing the quoted blob would both miss the real commands and
/// mistake quoted glob characters for a dynamic executable.
pub(super) fn strip_nested_script_quotes(script: &str) -> &str {
    for quote in ['\'', '"'] {
        if let Some(inner) = script.strip_prefix(quote).and_then(|rest| rest.strip_suffix(quote)) {
            return inner;
        }
    }
    script
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

    if let Some(script) = nested_shell_script(command)
        && let Ok(commands) =
            crate::command_safety::shell_parser::parse_shell_commands(strip_nested_script_quotes(script))
    {
        return commands.iter().any(|nested| command_requires_approval(nested));
    }

    false
}

fn executable_is_dynamic(executable: &str) -> bool {
    // `[` is the POSIX test builtin, not a glob; `[ -f path ]` must stay a
    // literal executable. Any other bracket usage is still dynamic.
    if executable == "[" {
        return false;
    }
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

pub(super) struct UnwrappedCommand<'a> {
    pub(super) command: &'a [String],
    pub(super) changes_directory: bool,
}

/// Programs that run another program given as their next argument.
///
/// Unwrapping them keeps a destructive command visible to the dangerous
/// detector and to the mounted-target approval, which would otherwise only
/// see the runner (`nohup rm -rf …`).
const PROGRAM_RUNNERS: &[&str] = &["builtin", "command", "exec", "nohup", "setsid", "time"];

/// Options that make `command`/`builtin` print a path instead of running it.
const LOOKUP_ONLY_OPTIONS: &[&str] = &["-p", "-v", "-V"];

pub(super) fn unwrap_command_prefix(command: &[String]) -> Option<&[String]> {
    unwrap_command_prefix_with_context(command).map(|unwrapped| unwrapped.command)
}

pub(super) fn unwrap_command_prefix_with_context(mut command: &[String]) -> Option<UnwrappedCommand<'_>> {
    let mut changes_directory = false;
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
                        changes_directory |= argument == "--chdir";
                        command = command.get(2..)?;
                    } else if matches!(argument, "-E" | "-H" | "-n" | "-S" | "-k" | "-K" | "-b")
                        || argument.starts_with("--user=")
                        || argument.starts_with("--group=")
                        || argument.starts_with("--host=")
                        || argument.starts_with("--chdir=")
                    {
                        changes_directory |= argument.starts_with("--chdir=");
                        command = &command[1..];
                    } else if argument.starts_with('-') {
                        return None;
                    } else {
                        break;
                    }
                }
            }
            name if PROGRAM_RUNNERS.contains(&name) => {
                let (options, program) = split_runner_options(command.get(1..)?);
                if matches!(name, "builtin" | "command")
                    && options.iter().any(|option| LOOKUP_ONLY_OPTIONS.contains(&option.as_str()))
                {
                    // `command -v prog` prints the path instead of running it.
                    return Some(UnwrappedCommand { command, changes_directory });
                }
                command = program;
            }
            _ => return Some(UnwrappedCommand { command, changes_directory }),
        }
    }
}

/// Split a runner's leading options from the program it runs.
fn split_runner_options(arguments: &[String]) -> (&[String], &[String]) {
    let mut index = 0;
    while index < arguments.len() && arguments[index].starts_with('-') {
        index += 1;
    }
    arguments.split_at(index)
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
mod tests;
