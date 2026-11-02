use super::*;
use serde_json::json;

fn run_cmd(command: &str) -> Value {
    json!({"action": "run", "command": command})
}

#[test]
fn command_availability_probes_do_not_admit_execution() {
    for command in ["command -v npx", "command -V python3 cargo", "command -v npx || true"] {
        assert!(is_readonly_command_session_command(&run_cmd(command)), "{command}");
    }
    for command in [
        "command rm file",
        "command -p rm file",
        "command -v",
        "command -v -- rm",
        "command -v $(touch file)",
    ] {
        assert!(!is_readonly_command_session_command(&run_cmd(command)), "{command}");
    }
}

#[test]
fn and_chain_allows_readonly_segments() {
    assert!(is_readonly_command_session_command(&run_cmd("ls -la && echo '---' && ls -la crates/")));
    assert!(is_readonly_command_session_command(&run_cmd("pwd && ls src/")));
    assert!(is_readonly_command_session_command(&run_cmd("cat foo.txt && grep bar")));
}

#[test]
fn compound_inspection_commands_share_the_readonly_policy() {
    for command in [
        "cat README.md; rg -n '^#' README.md",
        "git diff --stat; find docs -maxdepth 2 -type f | sort | head -40",
        "cat README.md\nrg -n '^version' Cargo.toml",
    ] {
        assert!(is_readonly_command_session_command(&run_cmd(command)), "expected readonly command: {command}");
    }
}

#[test]
fn checkpoint_style_inspection_with_escaped_regex_is_readonly() {
    let command = r#"sed -n '180,285p' src/main.rs; sed -n '60,285p' src/startup/mod.rs; sed -n '1,220p' src/main_helpers/bootstrap.rs; rg -n "\[profile|lto|codegen-units|strip" Cargo.toml"#;

    assert!(is_readonly_command_session_command(&run_cmd(command)));
}

#[test]
fn and_chain_rejects_destructive_segments() {
    assert!(!is_readonly_command_session_command(&run_cmd("ls -la && rm foo.txt")));
    assert!(!is_readonly_command_session_command(&run_cmd("cat x && mv a b")));
    assert!(!is_readonly_command_session_command(&run_cmd("true && cp a b")));
}

#[test]
fn and_chain_rejects_non_allowlisted_segments() {
    assert!(!is_readonly_command_session_command(&run_cmd("ls -la && python script.py")));
    assert!(!is_readonly_command_session_command(&run_cmd("ls -la && cargo build")));
    assert!(!is_readonly_command_session_command(&run_cmd("python3 mutate.py --dry-run")));
    assert!(!is_readonly_command_session_command(&run_cmd("npm install --dry-run && rm output")));
}

#[test]
fn and_chain_allows_pipeline_within_segment() {
    assert!(is_readonly_command_session_command(&run_cmd("ls -la | head -5 && echo done")));
}

#[test]
fn and_chain_single_command_passes() {
    assert!(is_readonly_command_session_command(&run_cmd("ls -la")));
    assert!(is_readonly_command_session_command(&run_cmd("echo hi")));
}

#[test]
fn readonly_command_session_allows_and_chain() {
    // The exact pattern from checkpoint turn_726 that was blocked.
    assert!(is_readonly_command_session_command(&run_cmd("ls -la /path/ && echo '---' && ls -la /path/crates/")));
}

#[test]
fn readonly_command_session_rejects_destructive_and_chain() {
    assert!(!is_readonly_command_session_command(&run_cmd("ls -la && rm foo.txt")));
}

#[test]
fn awk_range_print_is_readonly() {
    // Regression shapes from the blocked README session: `awk` paging
    // must not count as blind-editing mutations.
    for command in [
        "awk 'NR>=40 && NR<=140' README.md",
        "awk 'NR>=297 && NR<=312' README.md",
        "awk 'NR>=291 && NR<=296' README.md | cut -c1-150",
        "awk -F: '{print $1}' README.md",
        "awk -F, '{print $2}' data.csv",
        "awk -v limit=10 'NR<=limit' README.md",
        "awk -- '{print $1}' README.md",
        "awk -- '{print $1}' -- -weird",
        // `systime()` only reads the clock — must not be confused with
        // `system()`. Absolute paths resolve via file_name().
        "awk 'BEGIN{print systime()}' README.md",
        "/usr/bin/awk 'NR>=1 && NR<=5' README.md",
        "awk 'NR>=1 && NR<=5' README.md | sort",
        // Blocked-session regression: numeric `>` comparisons without
        // `print` redirection are reads, not writes.
        "awk '$3>100' README.md",
        "awk '{if(p2==0 && i>1) print}' README.md",
        "awk '{if(NR>1 && $2>10) print $1}' README.md",
        "awk 'NR>=65 && NR<=71 {n=length($0); print n}' README.md",
        "awk '{print (a>b)}' README.md",
    ] {
        assert!(is_readonly_command_session_command(&run_cmd(command)), "expected readonly command: {command}");
    }
}

#[test]
fn awk_write_primitives_stay_mutating() {
    for command in [
        "awk '{print > \"out.txt\"}' README.md",
        "awk '{print >> \"out.txt\"}' README.md",
        "awk '{print x > \"out.txt\"}' README.md",
        "awk 'BEGIN { # (\n print \"written\" > \"out.txt\" }'",
        "awk 'BEGIN { # [\n printf \"written\" > \"out.txt\" }'",
        "awk 'BEGIN { # ( [ {\n print \"written\" > \"out.txt\" }'",
        "awk '{print | \"sort\"}' README.md",
        "awk '\"sort\" | getline line' README.md",
        "awk 'BEGIN{system(\"touch out\")}' README.md",
        "awk 'BEGIN{SYSTEM (\"id\")}' README.md",
        "awk 'BEGIN{System(\"id\")}' README.md",
        "awk 'BEGIN{system\t(\"id\")}' README.md",
        // gawk indirect calls and directives can execute or load code,
        // including a `system` name smuggled via `-v`. Fail closed.
        "awk -v f=system 'BEGIN{@f(\"id\")}' README.md",
        "awk 'BEGIN{@s(\"id\")}' README.md",
        "awk '@include \"x.awk\"' README.md",
        "awk '@load \"ext\"' README.md",
        "awk '{print \"a@b\"}' README.md",
        // `|` alternations inside regex stay conservative without a full
        // parser; single `>` without `print` is now a comparison above.
        "awk '/error|warning/' README.md",
        "awk -i inplace '{print}' README.md",
        "awk -f program.awk README.md",
        "awk --source '{print}' README.md",
        "awk -l injail '{print}' README.md",
        "awk --profile '{print}' README.md",
        "awk -W dump-variables '{print}' README.md",
        "awk --posix '{print}' README.md",
        "awk -F:",
        "awk",
        "awk -v",
        // Shell-level redirection stays mutating even with a safe program.
        "awk 'NR>=1' README.md > out.txt",
    ] {
        assert!(!is_readonly_command_session_command(&run_cmd(command)), "expected mutating command: {command}");
    }
}

#[test]
fn awk_string_literal_operators_are_readonly() {
    // The reported false positive: a quoted `"|"` passed to `index()` is a
    // string literal, not a command pipe, so the lexer must not reject it.
    for command in [
        r#"awk '{n=index(rest,"|"); print n}' README.md"#,
        r#"awk 'index($0,">")' README.md"#,
        // `>=` comparison and `||` boolean operators are not redirection
        // or a pipe.
        r#"awk '$3>=100 && $4||$5 {print $1}' README.md"#,
        // Division must not be mistaken for a regex literal.
        r#"awk '{print $1 / $2}' README.md"#,
        "awk '{ # ( [ > system(\n if (NR>1) print $1 }' README.md",
        // Exact reported shape: a multi-line program whose `index(rest,"|")`
        // carries a quoted pipe and a `# ...` comment line.
        r#"awk 'NR>=208 && NR<=212 {line=$0; body=substr(line,1,length(line)-1); n=0; while (body ~ / \$/) { body=substr(body,1,length(body)-1); n++ }} # find guide start after label cell
rest=substr(line,3); g=index(rest,"|")+1; guide=substr(rest,g+2); gp=0; gg=guide; while (gg ~ / \$/) { gg=substr(gg,1,length(gg)-1); gp++ } print "%d: linelen=%d labelcell=%s pad_before_final_pipe=%d guide_pad=%d\n", NR, length(line), substr(line,3,20), n, gp }' README.md"#,
    ] {
        assert!(is_readonly_command_session_command(&run_cmd(command)), "expected readonly command: {command}");
    }
}

#[test]
fn awk_regex_with_quote_cannot_hide_pipe() {
    // Adversarial shape for a naive quote-toggling scanner: `/a"b/` opens
    // a "string" that stays open, hiding the real `|`; the later `/c"d/`
    // rebalances the quote count, so an unbalanced-quote guard never
    // fires. The lexer treats quotes as regex content and still catches
    // the pipe.
    for command in [
        r#"awk '/a"b/ {print | "sort"} /c"d/' README.md"#,
        // Regex alternation stays mutating (pinned conservative policy).
        r#"awk '/a|b/ {print}' README.md"#,
    ] {
        assert!(!is_readonly_command_session_command(&run_cmd(command)), "expected mutating command: {command}");
    }
}

#[test]
fn cd_prefixed_exploration_is_readonly() {
    // Exact patterns from checkpoint turn_810 that plan mode rejected:
    // the model habitually prefixes exploration with `cd <workspace> &&`.
    assert!(is_readonly_command_session_command(&run_cmd("cd /repo && sed -n '440,520p' Cargo.toml")));
    assert!(is_readonly_command_session_command(&run_cmd(
        "cd /repo && ls -la && echo '---' && rg --files -g 'Cargo.toml' | sed -n '1,120p'"
    )));
    // `cd` must not become a bypass for mutating chains.
    assert!(!is_readonly_command_session_command(&run_cmd("cd /repo && cargo build")));
    assert!(!is_readonly_command_session_command(&run_cmd("cd /repo && rm -rf target")));
}

#[test]
fn git_readonly_subcommands_allowed_in_chains_and_pipelines() {
    assert!(is_readonly_command_session_command(&run_cmd("git log --oneline | head -20")));
    assert!(is_readonly_command_session_command(&run_cmd("cd /repo && git diff")));
    assert!(is_readonly_command_session_command(&run_cmd("git show HEAD")));
    assert!(is_readonly_command_session_command(&run_cmd("git blame src/main.rs | head")));
    // Mutating git subcommands stay rejected.
    assert!(!is_readonly_command_session_command(&run_cmd("git checkout main")));
    assert!(!is_readonly_command_session_command(&run_cmd("cd /repo && git push")));
    assert!(!is_readonly_command_session_command(&run_cmd("git commit -m 'x'")));
}

#[test]
fn env_value_injection_is_not_readonly() {
    // A4: `command_words_after_environment_prefix` strips assignments, so
    // values must still be inspected. `GIT_CONFIG_*` reproduces the blocked
    // `git -c core.fsmonitor` injection without any `-c` token.
    for command in [
        "env GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0='touch /tmp/pwned' git status",
        "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0='touch /tmp/pwned' git status",
        "env GIT_CONFIG_GLOBAL=/tmp/evil.gitconfig git status",
        "env GIT_EXTERNAL_DIFF=evil git diff",
        "env GIT_TEXTCONV=evil git show HEAD",
        "env LD_PRELOAD=./evil.so ls",
        "env DYLD_INSERT_LIBRARIES=./evil.dylib ls",
        "env BASH_ENV=./evil.sh bash -lc 'echo hi'",
        "env NODE_OPTIONS='--require ./evil.js' node --version",
        "env EDITOR=evil sed -n '1,10p' README.md",
    ] {
        assert!(
            !is_readonly_command_session_command(&run_cmd(command)),
            "expected mutating env-injection command: {command}"
        );
    }

    // Ordinary env prefixes stay read-only.
    for command in [
        "env LANG=C.UTF-8 rg 'TODO' src",
        "FOO=bar git status",
        "env FOO=bar git log --oneline",
        "env -u GIT_CONFIG_COUNT git status",
        "env -C /repo git status",
    ] {
        assert!(
            is_readonly_command_session_command(&run_cmd(command)),
            "expected readonly env-prefixed command: {command}"
        );
    }
}

#[test]
fn git_worktree_and_history_plumbing_subcommands_are_readonly() {
    // turn-1030-class exploration: read-only plumbing subcommands the
    // plan gate used to deny outright.
    for command in [
        "git rev-list --count HEAD",
        "git ls-tree HEAD crates/",
        "git cat-file -p HEAD:Cargo.toml",
        "git diff-tree -r HEAD",
        "git merge-base main feat",
        "git range-diff main~2..main feat~2..feat",
        "git whatchanged -5",
        "git count-objects -v",
        "git reflog | head",
        "git worktree list",
        "git stash list",
        "git stash show",
        "git branch --show-current",
        "git --no-pager log --oneline | head -5",
        "git remote -v",
        "cd /repo && git -C . branch -a && git tag",
    ] {
        assert!(is_readonly_command_session_command(&run_cmd(command)), "expected readonly command: {command}");
    }
}

#[test]
fn version_and_help_probes_are_readonly_for_any_program() {
    for command in [
        "cargo --version",
        "node --version",
        "python3 -V",
        "npm --version",
        "git --version",
        "bash --help",
    ] {
        assert!(is_readonly_command_session_command(&run_cmd(command)), "expected readonly probe: {command}");
    }
    // Only an all-flags probe is a version/help query; real operands keep
    // the command outside the allow-list. Lowercase `-v` is excluded: it
    // means verbose for most tools and reads stdin for shells.
    assert!(!is_readonly_command_session_command(&run_cmd("python3 script.py --version")));
    assert!(!is_readonly_command_session_command(&run_cmd("git commit -v")));
    assert!(!is_readonly_command_session_command(&run_cmd("bash -v")));
}

#[test]
fn git_dash_c_repo_redirect_is_readonly_for_allowed_subcommands() {
    // `-C <dir>` only redirects which repository is read; plan mode must
    // not reject worktree-style exploration.
    assert!(is_readonly_command_session_command(&run_cmd("git -C .worktrees/feat log --oneline | head -5")));
    assert!(is_readonly_command_session_command(&run_cmd("git -C ../other rev-parse HEAD")));
    // Config injection (`-c`) stays denied — the `-C`/`-c` pair must not
    // be relaxed together.
    assert!(!is_readonly_command_session_command(&run_cmd("git -c core.pager=sh log")));
    // A read-only subcommand with a write option stays denied.
    assert!(!is_readonly_command_session_command(&run_cmd("git -C .worktrees/feat diff -o out.patch")));
}

#[test]
fn readonly_git_and_inspection_options_stay_fail_closed() {
    for command in [
        "git diff -o output.txt",
        "git diff '--output=output.txt'",
        "git diff -ooutput.txt",
        "git log --output=output.txt",
        "git show --textconv",
        // Config injection is a command-execution vector and stays denied.
        "git -c core.fsmonitor=touch status",
        "git --exec-path=/tmp/sh status",
        // Bare `git stash` is `git stash push` — it stashes uncommitted
        // work — and flag-only push forms carry no operand to betray
        // them. Only `list`/`show` are reads.
        "git stash",
        "git stash -u",
        "git stash -k",
        // Inline-value and glued short mutators are invisible to the
        // operand finder and must stay denied.
        "git branch --unset-upstream",
        "git branch --set-upstream-to=origin/main",
        "git branch --edit-description",
        "git branch -mnew-name",
        "git branch -u=origin/main",
        // An unknown value-taking global must not smuggle a mutating
        // subcommand past the allow-list.
        "git --shallow-file log commit -m x",
        // Operand forms of flag-shaped subcommands mutate and stay denied.
        "git tag v1.0.0",
        "git branch feature-x",
        "git branch -D stale",
        "git reflog expire --expire=now --all",
        "git stash pop",
        "git remote add origin https://example.com/r.git",
        "git worktree add ../side feat",
        "find . -fprint output.txt",
        "find . -fprintf output.txt '%p'",
        "rg --hostname-bin sh pattern",
        "rg --search-zip pattern",
        "rg -z pattern",
        "sort -o output.txt README.md",
        "sort --compress-program=sh README.md",
        "date -s now",
        "awk -i inplace '{print}' README.md",
        "sed -n 's/a/b/e' README.md",
        "fd --exec sh -c 'touch out'",
        "tree -o out.txt",
        "ast-grep -r 'README.md'",
        "sed -n -fmalicious.sed -e '1p' src/main.rs",
        "sed -I '' 's/a/b/' src/main.rs",
        "sed -n '1p\nw leaked.txt' src/main.rs",
        "sed -n 'woutput.txt' src/main.rs",
        "cargo check & rm output",
    ] {
        assert!(!is_readonly_command_session_command(&run_cmd(command)), "unexpected readonly command: {command}");
    }
}

#[test]
fn cargo_readonly_subcommands_allowed() {
    assert!(is_readonly_command_session_command(&run_cmd("cargo metadata")));
    assert!(is_readonly_command_session_command(&run_cmd("cargo tree | head -50")));
    assert!(is_readonly_command_session_command(&run_cmd("cargo nextest run")));
    assert!(!is_readonly_command_session_command(&run_cmd("cargo build")));
    assert!(!is_readonly_command_session_command(&run_cmd("cargo publish")));
}

#[test]
fn parallel_commands_are_limited_to_side_effect_free_inspection() {
    for command in ["rg -n TODO src", "cat Cargo.toml", "git status --short"] {
        assert!(is_parallel_safe_command_session_command(&run_cmd(command)), "expected parallel-safe: {command}");
    }
    for command in [
        "cargo check",
        "cargo test",
        "cargo nextest run",
        "cargo clippy",
        "npm test",
        "pnpm run test",
        "git diff | head -20",
    ] {
        assert!(
            !is_parallel_safe_command_session_command(&run_cmd(command)),
            "command can touch shared state or crosses a sequential boundary: {command}"
        );
    }
}

#[test]
fn shared_command_head_fails_closed_on_env_only_words() {
    // Asymmetric pair: env assignments alone leave no program to
    // classify (fail closed), while the same prefix with a program
    // classifies on both paths. Guards the shared helper's strip
    // order: prefix strip first, program resolution second.
    let env_only: Vec<String> = vec!["FOO=bar".to_string()];
    assert!(!command_words_are_readonly(&env_only));
    assert!(!command_words_are_parallel_safe(&env_only));
    assert!(readonly_command_head(&env_only).is_none());

    let env_ls: Vec<String> = vec!["FOO=bar".to_string(), "ls".to_string()];
    assert!(command_words_are_readonly(&env_ls));
    assert!(command_words_are_parallel_safe(&env_ls));
    let (stripped, first) = readonly_command_head(&env_ls).expect("env-prefixed ls classifies");
    assert_eq!(first, "ls");
    assert_eq!(stripped, ["ls".to_string()].as_slice());
}

#[test]
fn parallel_safe_path_rejects_env_injection_like_readonly() {
    // The shared head must pass the ORIGINAL words (env prefix
    // included) to the option guard. Stripped `["ls"]` looks clean;
    // only the unstripped view carries the injection.
    let injected: Vec<String> = vec!["LD_PRELOAD=./evil.so".to_string(), "ls".to_string()];
    assert!(!command_words_are_parallel_safe(&injected));
    assert!(!command_words_are_readonly(&injected));
    assert!(readonly_command_head(&injected).is_none());
}

#[test]
fn stderr_merge_is_not_a_write_redirection() {
    assert!(is_readonly_command_session_command(&run_cmd("cargo check 2>&1 | head -c 4000")));
    // A real file redirection is still rejected.
    assert!(!is_readonly_command_session_command(&run_cmd("cargo check > out.txt")));
}

#[test]
fn extra_inspection_base_commands_are_readonly() {
    assert!(is_readonly_command_session_command(&run_cmd("tree -L 2 crates/")));
    assert!(is_readonly_command_session_command(&run_cmd("fd -e rs planner")));
    assert!(is_readonly_command_session_command(&run_cmd("stat Cargo.toml && file target")));
    assert!(is_readonly_command_session_command(&run_cmd("which cargo")));
}

#[test]
fn spool_file_reads_are_detected_only_for_readonly_commands() {
    for command in [
        "cat .vtcode/context/tool_outputs/run-1.txt",
        "sed -n '1,20p' .vtcode/context/tool_outputs/run-1.txt",
        "rg error .vtcode/context/tool_outputs",
        "rg -n error .vtcode/context/tool_outputs | head -20",
        "tail -n 20 .vtcode/context/tool_outputs/run-1.txt",
    ] {
        assert!(is_spool_file_read_command("exec_command", &run_cmd(command)), "expected spool read: {command}");
    }

    for command in [
        "cat .vtcode/context/tool_outputs/run-1.txt > copied.txt",
        "sed -i 's/a/b/' .vtcode/context/tool_outputs/run-1.txt",
        "rm .vtcode/context/tool_outputs/run-1.txt",
        "cat \"$VTCODE_SPOOL\"",
        "cat .vtcode/context/tool_outputs/run-1.txt |",
        "cat notes-.vtcode/context/tool_outputs/run-1.txt",
        "cat .vtcode/context/tool_outputs-extra/run-1.txt",
    ] {
        assert!(!is_spool_file_read_command("exec_command", &run_cmd(command)), "unexpected spool read: {command}");
    }

    assert!(!is_spool_file_read_command("mcp_tool", &run_cmd("cat .vtcode/context/tool_outputs/run-1.txt")));
    assert!(is_spool_file_read_command("exec_command", &run_cmd("cat .vtcode/context/tool_outputs/run-1.txt")));
}

#[test]
fn spool_file_read_uses_one_parse_for_gate_and_scan() {
    // Equivalence with the old parse-twice form: env prefixes and
    // `&&` chains must resolve identically in the gate and the scan.
    // A dropped `all(readonly)` gate would flip the mutating row to
    // true (spooling a mutation); a reordered extract would flip the
    // env-prefixed row to false.
    assert!(is_spool_file_read_command(
        "exec_command",
        &run_cmd("FOO=bar cat .vtcode/context/tool_outputs/run-1.txt")
    ));
    assert!(is_spool_file_read_command(
        "exec_command",
        &run_cmd("cat .vtcode/context/tool_outputs/run-1.txt && echo done")
    ));
    assert!(!is_spool_file_read_command(
        "exec_command",
        &run_cmd("cat .vtcode/context/tool_outputs/run-1.txt && rm copied.txt")
    ));
}

#[test]
fn shared_prologue_keeps_classifier_gates_distinct() {
    // Same args through all three classifiers: outcomes must differ per
    // gate, proving the shared extract+parse did not collapse predicates.
    // Asymmetric by construction: each row flips a different gate.
    let spool_read = run_cmd("cat .vtcode/context/tool_outputs/run-1.txt");
    assert!(is_readonly_command_session_command(&spool_read));
    assert!(is_spool_file_read_command("exec_command", &spool_read));
    assert!(is_parallel_safe_command_session_command(&spool_read));

    // Build commands are policy-readonly but touch shared package-manager
    // state, so parallel safety stays denied while the spool gate (which
    // needs a spool path) also stays denied.
    let build = run_cmd("cargo check");
    assert!(is_readonly_command_session_command(&build));
    assert!(!is_parallel_safe_command_session_command(&build));
    assert!(!is_spool_file_read_command("exec_command", &build));

    // A mutating tail fails the shared read-only gate on both paths that
    // use it; the spool scan must never admit a mutation.
    let mutating_spool = run_cmd("cat .vtcode/context/tool_outputs/run-1.txt && rm copied.txt");
    assert!(!is_readonly_command_session_command(&mutating_spool));
    assert!(!is_spool_file_read_command("exec_command", &mutating_spool));

    // Conflicting raw overrides prove no single invocation: fail closed
    // on every classifier.
    let conflicted = json!({"action": "run", "command": "ls", "raw_command": "rm -rf /"});
    assert!(!is_readonly_command_session_command(&conflicted));
    assert!(!is_parallel_safe_command_session_command(&conflicted));
    assert!(!is_spool_file_read_command("exec_command", &conflicted));
}
