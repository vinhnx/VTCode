use serde_json::json;

use super::*;
use crate::config::constants::tools;
use crate::tools::tool_intent::is_readonly_command_session_command;

fn exec_command(command: &str) -> Value {
    json!({"cmd": command})
}

#[test]
fn literal_timers_do_not_create_mutation_debt_but_arbitrary_python_does() {
    let output = serde_json::json!({"exit_code":0});
    for command in [
        "python3 -c 'import time; time.sleep(0.01)'",
        "python -c 'import time; time.sleep(5)'",
        "sleep 30",
        "sleep 120",
        "sleep 0.01",
    ] {
        assert!(!exec_mutation_should_incur_debt(command, &output), "{command}");
        assert!(exec_mutation_should_incur_debt(command, &serde_json::json!({"modified_files":["src/lib.rs"]})));
    }
    for command in [
        "python3 -c 'import time; time.sleep(1); open(\"x\",\"w\")'",
        "python3 -c 'import time; time.sleep(float(\"nan\"))'",
        "python3 -c 'print(1)'",
        "python3 timer.py",
        "sleep $DELAY",
        "sleep 1 > timer.log",
        "sleep -1",
        "python3 -c 'import time; time.sleep(-1)'",
        "PYTHONSTARTUP=script.py python3 -c 'import time; time.sleep(1)'",
        "python3 -c 'import time; time.sleep(1)' > timer.log",
    ] {
        assert!(exec_mutation_should_incur_debt(command, &output), "{command}");
    }
}

#[test]
fn source_repair_commands_create_debt_instead_of_verifying() {
    for command in [
        "cargo clippy --fix --allow-dirty",
        "env LD_PRELOAD=./rewrite.so cargo clippy",
        "cargo +nightly clippy --fix",
        "cargo fix --allow-dirty",
        "ruff check --fix .",
        "ruff check --fix-only .",
        "ruff check --add-noqa .",
        "ruff check --add-ignore=reason .",
        "uv run --frozen --offline ruff check --fix .",
        "cargo clippy --fix && cargo check",
        "ruff check --fix . | head -20",
        // Do not treat an operand value as a safe preview flag.
        "ruff check --fix --exclude --diff .",
    ] {
        let args = exec_command(command);
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &args), ShellActivity::Mutation, "{command}");
        assert!(!shell_command_is_admitted_verification_attempt(&args), "{command}");
        assert!(exec_mutation_should_incur_debt(command, &json!({"exit_code": 0})), "{command}");
    }
    for command in [
        "cargo clippy --locked",
        "cargo nextest run fix",
        "ruff check --no-fix .",
        "ruff check --diff .",
        "uv run --frozen --offline ruff check --no-fix .",
    ] {
        let args = exec_command(command);
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &args), ShellActivity::Verification, "{command}");
        assert!(shell_command_is_admitted_verification_attempt(&args), "{command}");
        assert!(!exec_mutation_should_incur_debt(command, &json!({"exit_code": 0})), "{command}");
    }
}

#[test]
fn compiler_artifact_writes_remain_eligible_verification() {
    let command = "rustc --crate-type lib sample.rs -o checked.rlib";
    let args = exec_command(command);
    assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &args), ShellActivity::Verification);
    assert!(shell_command_is_admitted_verification_attempt(&args));
    // The inspection proof still sees a write; verifier eligibility owns
    // the build-artifact exemption, not the read-only permission boundary.
    assert!(exec_mutation_should_incur_debt(command, &json!({"exit_code":0})));
}

#[test]
fn verifier_diagnostics_recognize_masked_status_without_granting_admission() {
    for command in [
        "npx markdownlint-cli2 README.md; echo \"lint exit: $?\"",
        "cargo check; echo $?",
        "python3 scripts/check_markdown.py; echo \"lint exit: $?\"",
        "cargo check --locked || true",
        "cargo check; rm marker",
    ] {
        let args = exec_command(command);
        assert!(shell_command_contains_verifier(&args), "{command}");
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &args), ShellActivity::Mutation);
    }
    let masked = exec_command("npx markdownlint-cli2 README.md; echo \"lint exit: $?\"");
    assert!(!shell_command_is_admitted_verification_attempt(&masked));
    assert_eq!(shell_args_as_executed(tools::EXEC_COMMAND, &masked).as_ref(), &masked);
    for args in [
        exec_command("echo 'cargo check; echo $?'"),
        exec_command("grep 'cargo check' README.md"),
        exec_command("npx markdownlint-cli2 --fix README.md"),
        json!({"cmd":"cargo check; true", "raw_command":"echo hello"}),
        json!({"cmd":"npx markdownlint-cli2", "args":["--fix", "README.md"]}),
    ] {
        assert!(!shell_command_contains_verifier(&args), "{args}");
    }
}

#[test]
fn markdownlint_verification_preserves_command_boundaries() {
    for command in [
        "markdownlint-cli2 README.md",
        "npx --yes markdownlint-cli2 README.md",
        "npx --no-install markdownlint-cli2@0.23.3 README.md",
        "npx --yes --loglevel=error markdownlint-cli2@0.23.3 --config .markdownlint-cli2.jsonc ':README.md'",
        "npx -y markdownlint-cli2 README.md && cargo check --locked",
        "python3 scripts/check_markdown.py",
        "python ./scripts/check_markdown.py",
        "python3 scripts/check_markdown.py README.md",
        "python3 scripts/check_markdown.py docs/guide.md README.md",
        "python3 scripts/check_markdown.py ./README.md",
        "python3 scripts/check_docs_links.py",
        "python ./scripts/check_docs_links.py",
        "python3 scripts/check_docs_links.py README.md",
    ] {
        assert!(shell_command_is_admitted_verification_attempt(&exec_command(command)), "{command}");
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
            ShellActivity::Verification,
            "{command}"
        );
    }
    for command in [
        "markdownlint-cli2 --fix README.md",
        "npx --yes markdownlint-cli2 -f README.md",
        "npx --yes markdownlint-cli2 --fix=true README.md",
        "markdownlint-cli2 --version",
        "npx --package evil markdownlint-cli2 README.md",
        "npx --loglevel=error --package evil markdownlint-cli2 README.md",
        "npx --loglevel=error markdownlint-cli2 --fix README.md",
        "npx --loglevel=unknown markdownlint-cli2 README.md",
        "npx markdownlint-cli2@evil README.md",
        "npx markdownlint-cli2 README.md && rm README.md",
        "python3 scripts/check_markdown.py --fix",
        "python3 scripts/check_markdown.py --list",
        "python3 scripts/check_markdown.py --help",
        "python3 scripts/check_markdown.py README.md --fix",
        "python3 other/check_markdown.py",
        "python3 scripts/check_docs_links.py --help",
        "python3 other/check_docs_links.py",
    ] {
        assert!(!shell_command_is_admitted_verification_attempt(&exec_command(command)), "{command}");
    }
    assert_ne!(
        classify_shell_activity(tools::EXEC_COMMAND, &exec_command("npx markdownlint-cli2 README.md | grep error")),
        ShellActivity::Verification
    );
}

#[test]
fn output_search_recognizes_static_search_pipelines() {
    for command in [
        "rg --files --hidden -g '.markdownlint*'",
        "rg --files --hidden | rg -i 'markdownlint' | sed -n '1,15p'",
        "git ls-files | grep -i markdownlint | head -10",
        "LANG=C /usr/bin/grep -n missing README.md",
        "rg -- '-q' README.md",
        "rg -e -query src | head -10",
        "grep -ne-query README.md",
        "rg --regexp --quiet src",
        "rg -g '-q*' missing src",
        "rg '-g-q*' missing src",
        "rg -q --no-quiet missing src",
    ] {
        assert!(shell_command_is_output_search(&exec_command(command)), "{command}");
    }
    assert!(shell_command_is_output_search(&json!({"command": ["rg", "missing", "src"]})));
}

#[test]
fn output_search_excludes_quiet_probes_and_unproven_commands() {
    for command in [
        "rg -q pattern src",
        "grep -nq pattern README.md",
        "rg --quiet pattern src | head -10",
        "grep --silent pattern README.md",
        "rg -e -query -q src",
        "grep -nqe-query README.md",
        "rg --regexp --quiet --quiet src",
        "rg -q --no-quiet -q missing src",
        "echo 'rg missing'",
        "cat empty.txt",
        "cargo check | rg error",
        "rg missing src; touch result.txt",
        "rg missing $(pwd)",
        "rg missing src > result.txt",
        "rg --pre 'touch result.txt' missing src",
    ] {
        assert!(!shell_command_is_output_search(&exec_command(command)), "{command}");
    }
    assert!(!shell_command_is_output_search(&json!({})));
}

#[test]
fn output_search_distinguishes_separator_values_from_quiet_flags() {
    for option in [
        "--context-separator",
        "--field-context-separator",
        "--field-match-separator",
    ] {
        let command = format!("rg {option} -q missing src | head -10");
        assert!(shell_command_is_output_search(&exec_command(&command)), "{command}");
        let quiet_command = format!("rg {option} -q --quiet missing src | head -10");
        assert!(!shell_command_is_output_search(&exec_command(&quiet_command)), "{quiet_command}");
    }
}

#[test]
fn standalone_grep_search_preserves_exit_status_ownership() {
    for command in [
        "rg missing src",
        "grep -q missing README.md",
        "LANG=C /usr/bin/grep -n missing README.md",
        "rg 'a|b' src",
        "fgrep missing README.md",
    ] {
        assert!(shell_command_is_standalone_grep_search(&exec_command(command)), "{command}");
    }
    assert!(shell_command_is_standalone_grep_search(&json!({"command": ["rg", "missing", "src"]})));
    for command in [
        "rg missing src; false",
        "rg missing src && false",
        "rg missing src | grep other",
        "! rg missing src",
        "(rg missing src)",
        "echo '/rg missing'; false",
        "rg missing src 2>/dev/null",
        "rg missing src 2>&1",
        "rg missing $(pwd)",
        "rg --pre 'false' missing src",
    ] {
        assert!(!shell_command_is_standalone_grep_search(&exec_command(command)), "{command}");
    }
}

#[test]
fn admitted_verification_attempt_allows_truncation_but_blocks_smuggled_mutations() {
    for command in [
        "cargo check --locked 2>&1 | head -c 4000",
        "cargo check --locked",
        "cargo nextest run 2>&1 | head -c 4000",
    ] {
        assert!(
            shell_command_is_admitted_verification_attempt(&exec_command(command)),
            "expected admission: {command}"
        );
    }
    for command in [
        "cargo check && rm -rf target",
        "cargo check; rm foo.txt",
        "cargo check || rm foo.txt",
        "cargo check && cargo test && rm foo.txt",
        "sed -i '' 's/old/new/' README.md",
        "echo $(date)",
        "cargo check > build.log && rm foo.txt",
    ] {
        assert!(!shell_command_is_admitted_verification_attempt(&exec_command(command)), "expected block: {command}");
    }
    assert!(!shell_command_is_admitted_verification_attempt(&json!({})));
}

#[test]
fn logged_compound_inspection_commands_are_not_mutations() {
    for command in [
        "cat README.md && printf '\\n--- git status ---\\n' && git status --short",
        "wc -l README.md; rg -n '^#' README.md",
        "git diff --stat; find docs -maxdepth 2 -type f | sort | head -40",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
            ShellActivity::Inspection,
            "{command}"
        );
    }
}

#[cfg(unix)]
#[test]
fn captured_read_commands_with_output_suppression_are_inspection() {
    for command in [
        r#"sed -n '1,180p' README.md; sed -n '280,350p' README.md; sed -n '389,411p' README.md; printf '\n--- repo metadata ---\n'; git log -1 --format='%h %s'; sed -n '1,100p' Cargo.toml; rg -n '^version\s*=|rust-version|workspace\.package' Cargo.toml crates -g Cargo.toml | head -40"#,
        r#"sed -n '1,120p' crates/codegen/vtcode-core/src/tools/tool_intent/activity.rs; printf '\n--- readonly policy ---\n'; rg -n 'READONLY_UNIFIED_EXEC_COMMANDS|command_words_are_readonly' crates/codegen/vtcode-core/src/tools/tool_intent/readonly.rs; printf '\n--- recent commits ---\n'; git log -5 --oneline; printf '\n--- command arguments ---\n'; sed -n '1,180p' crates/codegen/vtcode-core/src/tools/command_args.rs"#,
        r###"git diff --stat; find docs -maxdepth 2 -type f | sort | head -40; rg -n "vtcode init|vtcode models|full-auto|run-debug|cargo install" docs/user-guide docs/installation docs/development 2>/dev/null | head -50"###,
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
            ShellActivity::Inspection,
            "{command}"
        );
    }
}

#[cfg(unix)]
#[test]
fn printf_output_safety_guards_remain_mutations() {
    for command in [
        "printf 'captured output\\n' > output.txt",
        "printf '%s\\n' \"$(git status --short)\"",
        "printf '%s\\n' `git status --short`",
        "printf '\\n--- inspection ---\\n' && rm output.txt",
    ] {
        let args = exec_command(command);
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &args), ShellActivity::Mutation, "{command}");
        assert!(!is_readonly_command_session_command(&args), "unexpected readonly command: {command}");
    }
}

#[test]
fn git_diff_check_chain_remains_inspection() {
    assert_eq!(
        classify_shell_activity(
            tools::EXEC_COMMAND,
            &exec_command("git diff --check && git status --short && git diff --stat"),
        ),
        ShellActivity::Inspection
    );
}

#[test]
fn verification_detection_skips_environment_prefixes() {
    for command in [
        "env RUSTFLAGS=-Dwarnings cargo check",
        "RUSTFLAGS=-Dwarnings env cargo check",
        "env -u PATH cargo check",
        "env -C /tmp cargo check",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
            ShellActivity::Verification,
            "{command}"
        );
    }
}

#[test]
fn ambiguous_or_mutating_compounds_remain_mutations() {
    for command in [
        "git diff --stat; python3 -c 'open(\"out\", \"w\").write(\"x\")'",
        "cat README.md; sed -i '' 's/a/b/' README.md",
        "sed --in-place= README.md",
        "git diff --output=out",
        "git diff '--output=out'",
        "git diff -o out",
        "git diff -oout",
        "git log --output=out",
        "git show --textconv",
        // `-C <dir> <read-only sub>` is an Inspection now (the redirect
        // only changes which repository is read); config injection stays
        // a Mutation.
        "git -c core.fsmonitor=touch status",
        "find . -fprint output.txt",
        "find . -fprintf output.txt '%p'",
        "rg --hostname-bin sh pattern",
        "rg --search-zip pattern",
        "rg -z pattern",
        "sort -o generated.txt README.md",
        "sort --compress-program=sh README.md",
        "date -s now",
        "awk -i inplace '{print}' README.md",
        "sed -n 's/a/b/e' README.md",
        "fd --exec sh -c 'touch output'",
        "tree -o output.txt",
        "ast-grep -r 'README.md'",
        "sed -n -fmalicious.sed -e '1p' src/main.rs",
        "sed -I '' 's/a/b/' src/main.rs",
        "sed -n '1p\nw leaked.txt' src/main.rs",
        "cargo check & rm output",
        "cargo check > build.log | rm output",
        "cargo check | head -40 > build.log",
        "cargo check | echo x > output.log",
        "cargo check | head -40",
        "cargo check > build.log &",
        "env -S 'cargo check'",
        "echo x > output.log && cargo check",
        "cargo check < build-input.log",
        "cat README.md > copied.txt",
        "git diff --check; rm output",
        "cat README.md\nrm output",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
            ShellActivity::Mutation,
            "{command}"
        );
    }
}

#[test]
fn quoted_output_text_does_not_change_inspection_classification() {
    for command in ["echo 'git diff --output=out'", "printf 'sort -o out input'"] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
            ShellActivity::Inspection,
            "{command}"
        );
    }
}

#[test]
fn cargo_fmt_check_is_verification_but_plain_fmt_is_not() {
    for command in [
        "cargo fmt --check",
        "cargo fmt --all -- --check",
        "cargo fmt -- --check",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
            ShellActivity::Verification,
            "{command}"
        );
        assert!(
            shell_command_is_admitted_verification_attempt(&exec_command(command)),
            "expected admission: {command}"
        );
    }
    // Plain `cargo fmt` rewrites the worktree: it must stay a mutation and
    // must not ride through the verification gate.
    assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &exec_command("cargo fmt")), ShellActivity::Mutation);
    assert!(!shell_command_is_admitted_verification_attempt(&exec_command("cargo fmt")));
}

#[test]
fn pure_and_chained_verifiers_are_verification() {
    for command in [
        "cargo fmt --all -- --check && cargo check --locked",
        "cargo check --locked && cargo nextest run --locked -p vtcode-ui",
        "cargo check --locked && cargo clippy --locked -p vtcode-ui -- -D warnings",
        "git diff --check && cargo check --locked",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
            ShellActivity::Verification,
            "{command}"
        );
        assert!(
            shell_command_is_admitted_verification_attempt(&exec_command(command)),
            "expected admission: {command}"
        );
    }
}

#[test]
fn non_and_chained_verifiers_remain_mutations() {
    for command in [
        "cargo check --locked; cargo nextest run --locked -p vtcode-ui",
        "cargo check --locked || cargo nextest run --locked -p vtcode-ui",
        "cargo check --locked | head -40",
        "cargo check --locked && cargo nextest run --locked -p vtcode-ui | head -40",
        "cargo check --locked &",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
            ShellActivity::Mutation,
            "{command}"
        );
    }
}

#[test]
fn expanded_verifiers_classify_as_verification() {
    for command in [
        "bun test",
        "bun run test",
        "deno lint",
        "deno check mod.ts",
        "make test",
        "make lint",
        "just verify",
        "ruff check src/",
        "ruff format --check",
        "tsc --noEmit",
        "eslint src/",
        "python3 -m pytest",
        "uv run pytest",
        "npm run lint",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
            ShellActivity::Verification,
            "{command}"
        );
    }
    for command in [
        "make clean",
        "make test clean",
        "just fmt",
        "tsc",
        "eslint --fix src/",
        "ruff format src/",
        "bun install",
        "uv sync",
    ] {
        assert_eq!(
            classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)),
            ShellActivity::Mutation,
            "{command}"
        );
    }
}

#[test]
fn escaped_quotes_fail_closed_instead_of_hiding_operators() {
    // A backslash-escaped quote is a literal for the shell, so the trailing
    // `;` is a live separator: the aggregate exit status can mask a failed
    // verifier, and the chain must not classify as pure `&&`.
    assert!(!shell_uses_only_and_chaining("cargo check --locked \\\"; echo ok"));
    assert!(!shell_uses_only_and_chaining("echo \\\" ; cargo check --locked && echo done"));
    // Escaped quotes inside real double quotes stay inert.
    assert!(shell_uses_only_and_chaining("echo \"a\\\"b\" && cargo check --locked"));
    assert!(shell_uses_only_and_chaining("echo \"path\" && cargo fmt --check"));
    // Backslash is literal inside single quotes.
    assert!(shell_uses_only_and_chaining("echo 'a\\b' && cargo check --locked"));
}

#[test]
fn default_verifier_prefers_manifest_priority_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(default_verifier_for_workspace(dir.path()), None);

    std::fs::write(dir.path().join("justfile"), "test:\n\techo ok\n").expect("justfile");
    assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("just test"));
    std::fs::write(dir.path().join("pyproject.toml"), "[tool.pytest]\n").expect("pyproject");
    assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("pytest -q"));
    std::fs::write(dir.path().join("package.json"), r#"{"scripts":{"test":"vitest"}}"#).expect("package.json");
    assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("npm test"));
    std::fs::write(dir.path().join("go.mod"), "module example\n").expect("go.mod");
    assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("go test ./..."));
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").expect("Cargo.toml");
    assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("cargo check --locked"));
}

#[test]
fn default_verifier_marker_aliases_and_tier_priority() {
    // Every table row resolves, including alternate spellings — a
    // dropped alias in the marker table must fail here, not in a
    // live session. Each fixture names a different row.
    for (marker, expected) in [
        ("Justfile", "just test"),
        ("makefile", "make test"),
        ("bun.lock", "bun test"),
        ("deno.jsonc", "deno test"),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(marker), "x\n").expect(marker);
        assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some(expected), "{marker}");
    }
    // Cross-tier priority: earlier tiers win over later ones.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("bun.lockb"), "x\n").expect("bun.lockb");
    std::fs::write(dir.path().join("deno.json"), "{}\n").expect("deno.json");
    assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("deno test"));
    std::fs::write(dir.path().join("go.mod"), "module example\n").expect("go.mod");
    assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("go test ./..."));
    // The Node tier (script sniffing) sits between Bun and Python.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("pyproject.toml"), "[tool.pytest]\n").expect("pyproject");
    std::fs::write(dir.path().join("package.json"), r#"{"scripts":{"test":"vitest"}}"#).expect("package.json");
    assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("npm test"));
    // Unrecognized scripts keep the `npm test` fallback.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("package.json"), r#"{"scripts":{"start":"serve"}}"#).expect("package.json");
    assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("npm test"));
}

#[test]
fn default_verifier_reads_npm_script_fallbacks() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("package.json"), r#"{"scripts":{"lint":"eslint ."}}"#).expect("package.json");
    assert_eq!(default_verifier_for_workspace(dir.path()).as_deref(), Some("npm run lint"));
}

#[test]
fn verification_recovery_directive_names_concrete_command() {
    let directive = verification_recovery_directive(Some("go test ./..."), 1, 2);
    assert!(directive.contains("go test ./..."));
    assert!(directive.contains("1/2"));
    assert!(directive.contains("edits remain available"));
    assert!(directive.contains("only missing checks"));
    assert!(!directive.contains("mutations are blocked"));
    assert!(directive.contains("max_output_tokens"));
    assert!(directive.contains(VERIFIER_SHELL_FORM_NOTE));
    assert!(directive.contains("tails run standalone"));
    assert!(directive.contains("fail-closed `pipefail`"));
    let fallback = verification_recovery_directive(None, 2, 2);
    assert!(fallback.contains(GENERIC_VERIFIER_DESCRIPTION));
    assert!(fallback.contains("2/2"));
}

#[test]
fn verifier_reference_names_resolved_command_or_generic_description() {
    assert_eq!(verifier_reference(Some("go test ./...")), "`go test ./...`");
    assert_eq!(verifier_reference(Some("  npm test ")), "`npm test`");
    for missing in [None, Some(""), Some("   ")] {
        let reference = verifier_reference(missing);
        assert_eq!(reference, GENERIC_VERIFIER_DESCRIPTION);
        assert!(!reference.starts_with('`'), "no single command is named: {reference}");
        for ecosystem in ["cargo check --locked", "go test ./...", "npm test", "pytest -q"] {
            assert!(reference.contains(ecosystem), "missing {ecosystem}: {reference}");
        }
    }
}

#[test]
fn truncation_only_verifier_rewrites_to_standalone_prefix() {
    for (command, expected) in [
        ("cargo check --locked 2>&1 | head -c 4000", "cargo check --locked 2>&1"),
        ("cargo check --locked -p vtcode 2>&1 | tail -20", "cargo check --locked -p vtcode 2>&1"),
        ("cargo nextest run 2>&1 | tail -15", "cargo nextest run 2>&1"),
        (
            "cargo check --locked && cargo nextest run --locked -p vtcode | tail -5",
            "cargo check --locked && cargo nextest run --locked -p vtcode",
        ),
        ("cargo check | tail -5 | head -20", "cargo check"),
        ("RUSTFLAGS=-Dwarnings cargo check | head -20", "RUSTFLAGS=-Dwarnings cargo check"),
        // File-output redirects are preserved verbatim in the head slice
        // (only the status-masking pipe tail is elided), and `>` does not
        // mask the exit status — so the rewrite stays truthful here too.
        ("cargo check > build.log | tail -5", "cargo check > build.log"),
    ] {
        assert_eq!(
            rewrite_truncation_only_verifier(&exec_command(command)).as_deref(),
            Some(expected),
            "expected rewrite: {command}"
        );
    }
}

#[test]
fn non_truncation_pipelines_and_mutations_are_never_rewritten() {
    // Filtering tails do real work; dropping them would discard evidence.
    // Smuggled mutations, joins, redirects, and array forms must run (or
    // be rejected) exactly as typed — the rewrite must stay silent.
    for command in [
        "cargo check | grep error",
        "cargo check | wc -l",
        "cargo check | sort | uniq",
        "cargo check && rm -rf target | tail -5",
        "cargo check; git status | tail -5",
        "cargo check || cargo test | tail -5",
        "cargo check | tail -5; rm foo.txt",
        "cargo check | tail &",
        "cargo check",
        "echo done | tail -5",
        "rg -n 'pattern' src | head -20",
        "echo $(date) | tail -5",
    ] {
        assert_eq!(rewrite_truncation_only_verifier(&exec_command(command)), None, "must not rewrite: {command}");
    }
    assert_eq!(
        rewrite_truncation_only_verifier(&serde_json::json!({"command": ["cargo", "check", "|", "tail"]})),
        None,
        "array-form commands keep today's behavior"
    );
    assert_eq!(rewrite_truncation_only_verifier(&serde_json::json!({})), None);
}

#[test]
fn args_as_executed_classify_elided_truncation_verifiers_as_verification() {
    for command in [
        "cargo check --locked 2>&1 | tail -5",
        "cargo nextest run 2>&1 | head -c 4000",
    ] {
        let typed = exec_command(command);
        let executed = shell_args_as_executed(tools::EXEC_COMMAND, &typed);
        assert!(matches!(executed, std::borrow::Cow::Owned(_)), "expected rewrite: {command}");
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &executed), ShellActivity::Verification, "{command}");
        // Idempotent: the executed form runs unchanged on a second pass.
        assert!(matches!(shell_args_as_executed(tools::EXEC_COMMAND, &executed), std::borrow::Cow::Borrowed(_)));
    }

    let unified = json!({"action": "run", "command": "cargo check 2>&1 | tail -5"});
    assert_eq!(shell_args_as_executed(tools::UNIFIED_EXEC, &unified)["command"], "cargo check 2>&1");
}

#[test]
fn verifier_rewrite_keeps_raw_alias_and_appended_arguments_in_sync() {
    let command = "cargo check | grep";
    let typed = json!({"cmd":command, "raw_command":command, "args":["warning"]});
    let executed = shell_args_as_executed(tools::EXEC_COMMAND, &typed);
    let expected = "set -o pipefail && cargo check | grep warning";
    assert_eq!(executed["command"], expected);
    assert_eq!(executed["raw_command"], expected);
    assert!(executed.get("args").is_none(), "suffix must execute exactly once");
    assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &executed), ShellActivity::Verification);
    assert!(matches!(shell_args_as_executed(tools::EXEC_COMMAND, &executed), std::borrow::Cow::Borrowed(_)));

    let truncator = json!({"cmd":"cargo check | head", "args":["-20"]});
    assert_eq!(shell_args_as_executed(tools::EXEC_COMMAND, &truncator)["command"], "cargo check");
}

#[test]
fn verifier_pipeline_rejects_conflicting_raw_alias_and_mutating_suffix() {
    for typed in [
        json!({"cmd":"cargo check | grep marker", "raw_command":"printf marker"}),
        json!({"cmd":"cargo check | sort", "args":["-o", "changed.txt"]}),
        json!({"cmd":"set -o pipefail && cargo check | sort", "args":["-o", "changed.txt"]}),
        json!({"cmd":"cargo check | grep", "args":"invalid"}),
    ] {
        assert_eq!(rewrite_verifier_pipeline(&typed), None, "{typed}");
        assert!(!shell_command_is_admitted_verification_attempt(&typed), "{typed}");
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &typed), ShellActivity::Mutation, "{typed}");
        assert!(super::super::classify_tool_intent(tools::EXEC_COMMAND, &typed).mutating, "{typed}");
    }
}

#[test]
fn filtered_verifier_pipeline_preserves_filters_and_truthful_status() {
    for command in [
        "python3 scripts/check_markdown.py 2>&1 | grep -E '^(README|docs/SECURITY)\\.md' | head -20",
        "cargo check --locked | grep warning",
        "cargo fmt --check && cargo check --locked | wc -l",
    ] {
        let typed = exec_command(command);
        let executed = shell_args_as_executed(tools::EXEC_COMMAND, &typed);
        assert_eq!(executed["command"], format!("set -o pipefail && {command}"));
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &executed), ShellActivity::Verification);
        assert!(shell_command_is_admitted_verification_attempt(&executed));
        assert!(matches!(shell_args_as_executed(tools::EXEC_COMMAND, &executed), std::borrow::Cow::Borrowed(_)));
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &typed), ShellActivity::Mutation);
    }
    for command in [
        "cargo check | grep error; true",
        "cargo check | grep error || true",
        "cargo check | tee source.rs",
        "cargo check | sed -i 's/old/new/' source.rs",
        "cargo check | grep $(touch marker)",
        "cargo check | grep error > hidden.log",
        "cargo check | grep error &",
        "cargo check && rm source.rs | grep error",
    ] {
        assert_eq!(rewrite_verifier_pipeline(&exec_command(command)), None, "{command}");
        let prefixed = exec_command(&format!("set -o pipefail && {command}"));
        assert_ne!(classify_shell_activity(tools::EXEC_COMMAND, &prefixed), ShellActivity::Verification, "{command}");
    }
}

#[test]
fn args_as_executed_keep_unsafe_joins_and_non_run_calls_as_typed() {
    for command in [
        "cargo check 2>&1 | grep error; true",
        "cargo check; git status",
        "cargo check || true",
    ] {
        let typed = exec_command(command);
        let executed = shell_args_as_executed(tools::EXEC_COMMAND, &typed);
        assert!(matches!(executed, std::borrow::Cow::Borrowed(_)), "must run as typed: {command}");
        assert_ne!(classify_shell_activity(tools::EXEC_COMMAND, &executed), ShellActivity::Verification, "{command}");
    }
    let poll = json!({"action": "poll", "session_id": "s1", "command": "cargo check | tail -5"});
    assert!(matches!(shell_args_as_executed(tools::UNIFIED_EXEC, &poll), std::borrow::Cow::Borrowed(_)));
    let read = json!({"path": "src/lib.rs"});
    assert!(matches!(shell_args_as_executed(tools::READ_FILE, &read), std::borrow::Cow::Borrowed(_)));
}

#[test]
fn path_glob_inspections_preserve_debt_without_weakening_admission() {
    let clean = json!({"exit_code":0});
    for command in [
        "rg -n \"MD013|line_length\" docs/development/*.md | head",
        "rg -n x ./docs/file?.md | head -20",
        "grep -n x docs/*.md",
        r#"grep -rn "default_webmcp\|WebmcpConfig" crates/codegen/vtcode-config/src --include=*.rs | head -10"#,
        "grep -rn x src --exclude=*.lock --exclude-dir=target*",
        "egrep x --include=*.rs src",
        "fgrep x --exclude=*.md src",
        "cat ./docs/*.md",
        "ls docs/*.md",
    ] {
        assert!(!exec_mutation_should_incur_debt(command, &clean), "{command}");
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)), ShellActivity::Mutation);
        assert!(exec_mutation_should_incur_debt(command, &json!({"exit_code":0,"modified_files":["src/lib.rs"]})));
    }
    for command in [
        "grep --include=$(touch marker) x src",
        "grep --exclude-dir=$directory x src",
        "grep --include=*.rs x src > marker",
        "grep --include=*.rs x src; touch marker",
        "rg --pre=touch --include=*.rs x src",
        "unknown --include=*.rs x src",
        "rg -n x *",
        "rg -n x ./docs/[a-z]*.md",
        "rg -n x -docs/*.md",
        "rg -n x $dir/*.md",
        "rg --pre touch docs/*.md",
        "rg --hostname-bin=touch docs/*.md",
        "sed docs/*.md",
        "sort -o marker docs/*.md",
        "./tools/* --version",
        "rg x docs/$(touch marker)/*.md",
        "rg x docs/*.md > marker",
        "rg x docs/*.md; touch marker",
        "rg x docs/*.md; if",
        "rg x docs*/*.md",
    ] {
        assert!(exec_mutation_should_incur_debt(command, &clean), "{command}");
    }
}

#[test]
fn mutation_accounting_rejects_unproven_shell_forms() {
    let output = json!({"exit_code": 0});
    for command in [
        "echo \"$(touch marker)\"",
        "echo \"`touch marker`\"",
        "for p in $(echo $(touch marker)); do test -f \"$p\"; done",
        "for p in a; do sed -i '' 's/a/b/' README.md; done",
        "for p in a; do sort -o README.md README.md; done",
        "echo x 2>/dev/null.backup",
        "echo x 2>&1>marker",
        "cat <(touch marker)",
        "$program --version",
        "\"$(echo rm)\" marker",
        "grep $options README.md",
        "if true; then touch marker; fi",
        "for p in a; do echo x",
        "echo $(",
        "echo hi &",
        "sed -n '1p' README.md > marker",
        "echo ${x:=value}",
    ] {
        assert!(exec_mutation_should_incur_debt(command, &output), "{command}");
    }
    for command in [
        "echo '>'",
        "echo '2>/dev/null.backup'",
        "echo \"$(grep -n x README.md)\"",
    ] {
        assert!(!exec_mutation_should_incur_debt(command, &output), "{command}");
    }
}

#[test]
fn dynamic_readonly_inspections_do_not_incur_mutation_debt() {
    let clean_output = json!({"success": true, "exit_code": 0});
    // Session-vtcode-20261015T142625Z: `for` loops over grep results with
    // `test -f` exited 0 with no writes, bumped the completion generation,
    // invalidated tracker evidence, and looped harness auto-verification
    // on `rg` inspections instead of ending the turn. Admission stays
    // fail-closed (still Mutation, still blocked when pending); accounting
    // must not create new debt.
    for command in [
        "for p in $(grep -o '](\\./[^)#]*' README.md | sed 's/](//' | sort -u); do test -f \"$p\" || echo \"missing: $p\"; done; echo \"check done\"",
        "for p in $(grep -o '](\\./[^)#]*' README.md | sed 's/](\\.\\///' | sort -u); do test -f \"docs/${p#docs/}\" 2>/dev/null || echo \"missing: $p\"; done",
        "rg -n 'vinhnx/(VTCode|vtcode)' README.md",
        "grep -n '^## ' README.md",
        "ls docs/guides/zed-acp.md",
        "git diff --stat -- README.md",
        "git status --short",
        "npx markdownlint-cli2 README.md",
        "cargo check --locked",
    ] {
        let expected = if command.starts_with("npx markdownlint") || command.starts_with("cargo check") {
            ShellActivity::Verification
        } else if command.starts_with("for") {
            ShellActivity::Mutation
        } else {
            ShellActivity::Inspection
        };
        assert_eq!(classify_shell_activity(tools::EXEC_COMMAND, &exec_command(command)), expected, "{command}");
        assert!(!exec_mutation_should_incur_debt(command, &clean_output), "{command}");
    }
    // Dynamic `for`/`test` inspections: no debt without writes.
    assert!(!exec_mutation_should_incur_debt(
        "for p in $(grep -o 'x' README.md); do test -f \"$p\" || echo \"missing: $p\"; done",
        &clean_output,
    ));
    assert!(!exec_mutation_should_incur_debt("rg -n 'x' README.md", &clean_output));
    assert!(!exec_mutation_should_incur_debt("grep -n 'x' README.md", &clean_output));
    assert!(!exec_mutation_should_incur_debt("git status --short", &clean_output));
    // Concrete evidence still counts.
    assert!(exec_mutation_should_incur_debt(
        "for p in $(grep -o 'x' README.md); do test -f \"$p\" || echo \"missing: $p\"; done",
        &json!({"success": true, "exit_code": 0, "modified_files": ["README.md"]}),
    ));
    // Shell redirection is a potential write.
    assert!(exec_mutation_should_incur_debt("echo hi > README.md", &clean_output));
    assert!(exec_mutation_should_incur_debt("for f in a b; do echo x > \"$f\"; done", &clean_output));
    // Static mutating base still counts (fail-closed for `rm`, etc.).
    assert!(exec_mutation_should_incur_debt("rm -rf target", &clean_output));
    // Dynamic mutating base with variable still counts.
    assert!(exec_mutation_should_incur_debt("rm $f", &clean_output));
    // Stderr plumbing alone is not a file write.
    assert!(!exec_mutation_should_incur_debt("rg -n 'x' README.md 2>&1", &clean_output));
    // Shared classifiers decide verifier-shaped commands: mutating
    // variants count, clean checks do not. Asymmetric pairs.
    assert!(exec_mutation_should_incur_debt("npx markdownlint-cli2 --fix README.md", &clean_output));
    assert!(!exec_mutation_should_incur_debt("npx markdownlint-cli2 README.md", &clean_output));
    assert!(exec_mutation_should_incur_debt("cargo fmt", &clean_output));
    assert!(!exec_mutation_should_incur_debt("cargo fmt --check", &clean_output));
    assert!(exec_mutation_should_incur_debt("tsc", &clean_output));
    assert!(!exec_mutation_should_incur_debt("tsc --noEmit", &clean_output));
    // Unresolved options stay unproven even for inspection subcommands.
    assert!(exec_mutation_should_incur_debt("git diff $x", &clean_output));
    assert!(exec_mutation_should_incur_debt("git checkout $x", &clean_output));
    // Dynamic chains of inspections with exit-echo tails stay debt-free.
    assert!(!exec_mutation_should_incur_debt("grep -n 'x' README.md; echo \"exit:$?\"", &clean_output,));
}
