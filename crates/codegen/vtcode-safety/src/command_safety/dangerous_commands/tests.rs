use super::*;

fn vec_str(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

#[test]
fn nested_shell_script_is_analyzed_without_its_quoting() {
    // Raw argument text keeps the shell quotes, but the shell strips them
    // before running the script: the quoted blob must not be treated as an
    // executable, and the commands inside it must still be inspected.
    for script in [
        "'if [ -t 1 ]; then echo TTY_YES; else echo TTY_NO; fi; echo MARKER; exit 3'",
        "\"if [ -t 1 ]; then echo TTY_YES; fi\"",
        "'echo $HOME'",
        "'printf %s hello; printf %s world'",
    ] {
        let command = vec_str(&["sh", "-c", script]);
        assert!(!command_might_be_dangerous(&command), "benign nested script: {command:?}");
    }

    for script in ["'git status && rm -rf /'", "'git reset --hard HEAD~1'"] {
        let command = vec_str(&["bash", "-c", script]);
        assert!(command_might_be_dangerous(&command), "destructive nested script: {command:?}");
    }
}

#[test]
fn nested_shell_script_rejects_command_substitution() {
    // The outer single quotes make `$(…)`/backticks inert for the
    // top-level scan; the inner shell still runs them. `echo` and `touch`
    // are not dangerous on their own, so only the substitution check can
    // reject these.
    for script in ["'echo $(touch /tmp/probe)'", "'printf %s `touch /tmp/probe`'"] {
        let command = vec_str(&["sh", "-c", script]);
        assert!(command_might_be_dangerous(&command), "nested substitution must be rejected: {command:?}");
    }
}

#[test]
fn nested_shell_script_keeps_inline_code_in_approval_classification() {
    // Inline interpreter code is admitted only through sandbox or explicit
    // approval. Quoted nested scripts must not escape that classification:
    // parsing the raw `'…'` blob would hide the inner `python3 -c`.
    let command = vec_str(&["sh", "-c", "'python3 -c \"print(1)\"'"]);
    assert!(command_requires_approval(&command), "quoted nested inline code must require approval: {command:?}");
    // Benign nested scripts still pass without an approval prompt.
    let benign = vec_str(&["sh", "-c", "'echo MARKER'"]);
    assert!(!command_requires_approval(&benign), "benign nested script: {benign:?}");
}

#[test]
fn posix_test_builtin_is_not_a_dynamic_executable() {
    assert!(!command_might_be_dangerous(&vec_str(&["[", "-t", "1", "]"])));
    assert!(!command_might_be_dangerous(&vec_str(&["[", "-f", "src/main.rs", "]"])));
    // Glob-shaped executables stay dynamic.
    assert!(command_might_be_dangerous(&vec_str(&["[abc]", "x"])));
    assert!(command_might_be_dangerous(&vec_str(&["{cmd}", "x"])));
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

/// A launcher runs another program; the destructive command behind it must
/// stay visible to the detector.
#[test]
fn launcher_prefixes_cannot_hide_a_destructive_command() {
    for runner in ["nohup", "setsid", "time", "exec", "command", "builtin"] {
        for destructive in [
            vec!["rm", "-rf", "/"],
            vec!["rm", "-f", "file.txt"],
            vec!["git", "reset", "--hard", "HEAD~1"],
            vec!["dd", "if=/dev/zero", "of=/dev/sda"],
        ] {
            let mut command = vec![runner.to_string()];
            command.extend(destructive.iter().map(|word| (*word).to_string()));
            assert!(command_might_be_dangerous(&command), "{runner} must not hide a destructive command: {command:?}");
        }
    }
}

/// A lookup prints a path instead of running the program, so it must not
/// be classified as if the program had run.
#[test]
fn command_lookup_options_do_not_run_the_program() {
    for command in [
        vec!["command", "-v", "rm"],
        vec!["command", "-V", "rm"],
        vec!["builtin", "-p", "rm"],
    ] {
        let command: Vec<String> = command.into_iter().map(str::to_owned).collect();
        assert!(
            !command_might_be_dangerous(&command),
            "a path lookup must not be classified as the program: {command:?}"
        );
    }
}

/// Runner options are skipped so the program they run is still found.
#[test]
fn runner_options_are_skipped_before_the_program() {
    for command in [
        vec!["nohup", "--", "rm", "-rf", "/"],
        vec!["time", "-p", "rm", "-rf", "/"],
    ] {
        let command: Vec<String> = command.into_iter().map(str::to_owned).collect();
        assert!(command_might_be_dangerous(&command), "runner options must not hide the program: {command:?}");
    }
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
