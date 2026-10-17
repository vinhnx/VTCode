use super::steps::parse_bracket_list;
use super::verification::{is_independent_rederivation, validate_concrete_verification};
use super::{split_bracket_items, validate_plan_content};

#[test]
fn rust_lifetime_prose_keeps_metadata_boundaries() {
    for action in [
        "Change return type to &'static str",
        "Use Foo<'a, 'b>",
        "Use Foo< 'a >",
        "Use Foo< 'a, 'b >",
        "Add T: 'static bound",
        "Use T: 'a + 'b bounds",
        "Explain example: 'input -> files: []'",
        "Explain example + 'input -> files: []'",
    ] {
        let plan = format!(
            "## Summary\nUpdate return type.\n\n## Implementation Steps\n1. {action} -> files: [src/lib.rs] -> verify: [cargo check]\n\n## Test Cases and Validation\nRun cargo check.\n\n## Assumptions and Defaults\nPreserve callers."
        );
        assert!(validate_plan_content(&plan).is_ready(), "must accept {action}");
        let tracker = super::generate_tracker_markdown_from_plan(&plan).expect("tracker");
        assert!(tracker.contains(&format!("- [ ] {action}\n")));
        assert!(tracker.ends_with("  verify: cargo check"));
    }
}

#[test]
fn skipped_markdown_lint_does_not_replace_code_verification() {
    let plan = "## Summary\nUpdate implementation and documentation.\n\n## Implementation Steps\n1. Update targets -> files: [src/lib.rs, README.md] -> verify: [skip Markdown lint if unavailable]\n\n## Test Cases and Validation\nReview links.\n\n## Assumptions and Defaults\nPreserve behavior.";
    assert_eq!(validate_plan_content(plan).invalid_implementation_steps.len(), 1);
    let with_check =
        plan.replace("[skip Markdown lint if unavailable]", "[cargo check, skip Markdown lint if unavailable]");
    assert!(validate_plan_content(&with_check).is_ready());
    for targets in [
        "README.md",
        "docs/guide.markdown",
        "README.md:42",
        "C:\\docs\\README.md",
    ] {
        let documentation = plan.replace("src/lib.rs, README.md", targets);
        assert!(validate_plan_content(&documentation).is_ready(), "must accept {targets}");
    }
    let code_only = plan.replace("src/lib.rs, README.md", "src/lib.rs");
    assert_eq!(validate_plan_content(&code_only).invalid_implementation_steps.len(), 1);
    let trailing_code = plan
        .replace("src/lib.rs, README.md", "README.md")
        .replace("if unavailable]", "if unavailable] -> files: [src/lib.rs]");
    assert_eq!(validate_plan_content(&trailing_code).invalid_implementation_steps.len(), 1);
    let trailing_bad_check = with_check.replace("if unavailable]", "if unavailable] -> verify: [check later]");
    assert_eq!(validate_plan_content(&trailing_bad_check).invalid_implementation_steps.len(), 1);
}

#[test]
fn step_metadata_preserves_quoted_arrows_and_trailing_outcome() {
    let plan = "## Summary\nDocument arrow syntax.\n\n## Implementation Steps\n1. Explain `input -> files: []` and don't truncate the user's example -> files: [README.md] -> verify: [rg -n 'input -> outcome: [example]' README.md] -> outcome: [examples remain visible]\n\n## Test Cases and Validation\nReview examples.\n\n## Assumptions and Defaults\nDocumentation only.";
    assert!(validate_plan_content(plan).is_ready());
    let tracker = super::generate_tracker_markdown_from_plan(plan).expect("tracker");
    assert!(tracker.contains("- [ ] Explain `input -> files: []` and don't truncate the user's example\n"));
    assert!(tracker.contains("  files: README.md\n"));
    assert!(tracker.contains("  verify: rg -n 'input -> outcome: [example]' README.md\n"));
    assert!(tracker.ends_with("\n  outcome: examples remain visible"));
}

#[test]
fn readme_plan_preserves_prose_arrows_and_quoted_verification() {
    let plan = "## Summary\nRefine README wording.\n\n## Implementation Steps\n1. Arrange Overview → Quick start → Workflows; update contents -> files: [README.md] -> verify: [rg -n '^## |^### ' README.md]\n2. Retain install → configure → launch -> files: [README.md] -> verify: [git diff --word-diff=plain -- README.md]\n3. Explain input -> output -> files: [README.md] -> verify: [rg -n 'input -> output → files: example' README.md]\n\n## Test Cases and Validation\nContents links match headings.\n\n## Assumptions and Defaults\nPreserve user edits.";
    let report = validate_plan_content(plan);
    assert!(report.is_ready(), "session-shaped plan must validate: {report:?}");
    let tracker = super::generate_tracker_markdown_from_plan(plan).expect("tracker");
    assert!(tracker.contains("- [ ] Arrange Overview → Quick start → Workflows; update contents\n"));
    assert!(tracker.contains("- [ ] Retain install → configure → launch\n"));
    assert!(tracker.contains("- [ ] Explain input -> output\n"));
    assert_eq!(tracker.matches("  files: README.md\n").count(), 3);
    assert!(tracker.contains("  verify: rg -n 'input -> output → files: example' README.md"));

    let invalid = plan.replace("files: [README.md]", "files: []");
    assert_eq!(validate_plan_content(&invalid).invalid_implementation_steps.len(), 3);
}

#[test]
fn optional_markdown_check_requires_unavailable_condition() {
    for verify in [
        "skip Markdown lint if unavailable",
        "skip Markdown validation if unavailable",
    ] {
        assert!(validate_concrete_verification(verify).is_ok());
        let plan = format!(
            "## Summary\nRefine documentation.\n\n## Implementation Steps\n1. Review README.md -> files: [README.md] -> verify: [{verify}]\n\n## Test Cases and Validation\nReview local links with available tools.\n\n## Assumptions and Defaults\nDo not install optional lint tooling."
        );
        assert!(validate_plan_content(&plan).is_ready());
    }
    for verify in ["skip validation", "skip Markdown lint", "skip tests if unavailable"] {
        assert!(validate_concrete_verification(verify).is_err(), "must reject {verify}");
    }
}

#[test]
fn readme_markdown_verification_list_accepts_npx() {
    let checks = "[npx markdownlint-cli2 README.md, python3 scripts/check_markdown_location.py, git diff -- README.md]";
    assert!(validate_concrete_verification(checks).is_ok());
    assert!(validate_concrete_verification("npx").is_err());
    let plan = format!(
        "## Summary\nRefine README structure and wording.\n\n## Implementation Steps\n1. Update heading anchors and validate Markdown -> files: [README.md] -> verify: {checks}\n\n## Test Cases and Validation\nContents links resolve to their headings.\n\n## Assumptions and Defaults\nChange README.md only."
    );
    assert!(validate_plan_content(&plan).is_ready());
    let tracker = super::generate_tracker_markdown_from_plan(&plan).expect("tracker");
    assert!(tracker.contains("verify: npx markdownlint-cli2 README.md"));
    assert!(tracker.contains("verify: python3 scripts/check_markdown_location.py"));
    assert!(tracker.contains("verify: git diff -- README.md"));
    assert!(validate_concrete_verification("[npx markdownlint-cli2 README.md, check later]").is_err());
}

#[test]
fn targeted_git_history_verifies_review_plan() {
    let plan = "## Summary\nReview the completed background-task work.\n\n## Implementation Steps\n1. Review completion changes -> files: [src/agent/runloop/unified/turn/session_loop_runner/background_completion.rs] -> verify: [git log -5 --oneline]\n2. Inspect the shipped commit -> files: [src/agent/runloop/unified/turn/session_loop_runner/support.rs] -> verify: [git show --stat HEAD]\n3. Compare the prior version -> files: [src/agent/runloop/unified/turn/session_loop_runner/support.rs] -> verify: [git diff HEAD~1 -- src/agent/runloop/unified/turn/session_loop_runner/support.rs]\n4. Trace ownership -> files: [src/agent/runloop/unified/turn/session_loop_runner/support.rs] -> verify: [git blame src/agent/runloop/unified/turn/session_loop_runner/support.rs]\n\n## Test Cases and Validation\nInspect selected history.\n\n## Assumptions and Defaults\nNo file edits are expected.";
    let report = validate_plan_content(plan);
    assert!(report.is_ready(), "targeted history checks should validate: {report:?}");
    assert!(validate_concrete_verification("run git show --stat HEAD").is_ok());
}

#[test]
fn git_verification_rejects_mutation_and_unfocused_checks() {
    for verify in [
        "git log",
        "git show --stat",
        "git diff",
        "git diff --check",
        "git diff --check HEAD",
        "git log --grep=",
        "git reset --hard HEAD",
        "git checkout main",
        "git show HEAD; git reset --hard HEAD",
        "please run git reset --hard HEAD",
        "/usr/bin/git reset --hard HEAD",
        "command /usr/bin/git reset --hard HEAD",
        "git show HEAD\ngit reset --hard HEAD",
        "cargo check && git reset --hard HEAD",
        "cargo check; git reset --hard HEAD",
        "cargo check;git reset --hard HEAD",
        "cargo check\ngit reset --hard HEAD",
        "review history",
    ] {
        assert!(validate_concrete_verification(verify).is_err(), "must reject {verify}");
    }

    for verify in ["run git show --stat HEAD", "/usr/bin/git show --stat HEAD"] {
        assert!(validate_concrete_verification(verify).is_ok(), "must accept {verify}");
    }
}

#[test]
fn independent_rederivation_counts_as_concrete_verification() {
    let verify = "independent-rederive bitstream order (fresh context, no helper reuse)";
    assert!(is_independent_rederivation(verify));
    assert!(validate_concrete_verification(verify).is_ok());
}

#[test]
fn vague_verify_without_fresh_rederivation_still_rejected() {
    assert!(!is_independent_rederivation("verify later"));
    assert!(validate_concrete_verification("verify later").is_err());
}

#[test]
fn vacuous_independent_compare_without_rederive_still_rejected() {
    // `compare` alone is manual verification (needs evidence), not an
    // independent re-derivation oracle.
    assert!(!is_independent_rederivation("independent compare fresh context"));
    assert!(!is_independent_rederivation("independent rederive fresh"));
}

#[test]
fn bracket_split_keeps_quoted_commas_inside_one_item() {
    let items = split_bracket_items("rg -n 'a,b' README.md, cargo check --locked");
    assert_eq!(items.len(), 2);
    assert!(items[0].contains("rg -n"));
    assert!(items[1].contains("cargo check"));

    // Session session-vtcode-20260914T031505Z_075199-09813 step 4: the
    // comma inside the quoted sed range must not split the item.
    let single =
        parse_bracket_list("[sed -n '/^## Documentation/,/^## Development/p' README.md | wc -l reports fewer lines]");
    assert_eq!(single.len(), 1);
    assert!(
        validate_concrete_verification(&single[0]).is_ok(),
        "quoted-comma sed inspection command must be one concrete verify item"
    );

    let concrete = parse_bracket_list("[rg -n 'a,b' README.md]");
    assert_eq!(concrete.len(), 1);
    assert!(validate_concrete_verification(&concrete[0]).is_ok());
}

#[test]
fn inspection_commands_count_as_concrete_verification() {
    // Live planning drafts (checkpoints turn_1234–1243) emitted sed/grep
    // verifies for README/docs changes and were rejected as
    // "verification item 1 must be a concrete command or check".
    for verify in [
        "sed -n '81,88p' README.md",
        "grep -n 'planning-workflow' docs/guides/planning-workflow.md",
        "head -40 docs/file.md",
        "tail -20 docs/file.md",
        "wc -l README.md",
        "wc README.md",
        "head README.md",
        "file src/main.rs",
        "ls src/",
        "cargo test",
    ] {
        assert!(
            validate_concrete_verification(verify).is_ok(),
            "inspection command should validate as concrete: {verify}"
        );
    }

    for invalid in [
        "run checks",
        "check later",
        "git diff --check",
        "review docs",
        // Ambiguous English command heads without flag/path evidence.
        "file changes",
        "sort order",
        "find files",
        "make sense",
        "ls files",
    ] {
        assert!(
            validate_concrete_verification(invalid).is_err(),
            "vague or VCS-only verify must stay rejected: {invalid}"
        );
    }
}

#[test]
fn bracket_split_still_splits_unquoted_items_and_flags_vague_first_item() {
    let items = parse_bracket_list("[review docs, cargo check --locked]");
    assert_eq!(items.len(), 2);
    assert!(validate_concrete_verification(&items[0]).is_err());
    assert!(validate_concrete_verification(&items[1]).is_ok());
}

#[test]
fn per_item_brackets_in_verify_list_validate_as_one_plan() {
    // Live session session-vtcode-20261011T125442Z_154501-05568 step 3:
    // the model emitted each verification command in its own bracket group
    // (`verify: [cmd_a], [cmd_b]`). The old parser stripped only the outer
    // pair before splitting, producing the fragments `cmd_a]` and `[cmd_b`,
    // and item 2 failed as "verification item 2 must be a concrete command or
    // check", terminally rejecting an otherwise valid README plan.
    let verify = "[npx markdownlint-cli2 README.md], [git diff -- README.md]";
    let items = parse_bracket_list(verify);
    assert_eq!(items, vec!["npx markdownlint-cli2 README.md", "git diff -- README.md"]);
    for item in &items {
        assert!(validate_concrete_verification(item).is_ok(), "each per-item bracket command must validate: {item}");
    }
    assert!(validate_concrete_verification(verify).is_ok());

    let plan = format!(
        "## Summary\nRefine README.\n\n## Implementation Steps\n1. Refine intro -> files: [README.md] -> verify: [git diff -- README.md]\n2. Check links -> files: [README.md, docs/INDEX.md] -> verify: [python3 scripts/check_markdown_location.py]\n3. Review format -> files: [README.md] -> verify: {verify}\n\n## Test Cases and Validation\nRun checks.\n\n## Assumptions and Defaults\nREADME only."
    );
    assert!(validate_plan_content(&plan).is_ready());
}

#[test]
fn bracket_split_handles_backslash_escapes_outside_single_quotes() {
    // Escaped comma outside quotes stays inside one item.
    let items = split_bracket_items("foo\\,bar, baz");
    assert_eq!(items, vec!["foo\\,bar".to_string(), " baz".to_string()]);

    // Escaped quote inside double quotes neither toggles nor splits.
    let items = split_bracket_items("rg -n \"a\\\"b,c\", cargo check");
    assert_eq!(items.len(), 2);
    assert!(items[0].contains("rg -n"));
    assert!(items[1].contains("cargo check"));
}

#[test]
fn parse_bracket_list_supports_whole_group_and_per_item_forms() {
    // One outer group wrapping the whole list keeps the legacy behavior.
    assert_eq!(parse_bracket_list("[src/a.rs, src/b.rs]"), vec!["src/a.rs", "src/b.rs"]);
    // Per-item groups each unwrap to their own entry.
    assert_eq!(parse_bracket_list("[cmd_a], [cmd_b]"), vec!["cmd_a", "cmd_b"]);
    // Mixed per-item group followed by a bare value.
    assert_eq!(parse_bracket_list("[cmd_a], cmd_b"), vec!["cmd_a", "cmd_b"]);
    // Bare comma-separated values with no brackets.
    assert_eq!(parse_bracket_list("src/a.rs, src/b.rs"), vec!["src/a.rs", "src/b.rs"]);
    // A comma nested inside a per-item group never fragments the entry.
    assert_eq!(
        parse_bracket_list("[rg -n 'a,b' README.md], [cargo check]"),
        vec!["rg -n 'a,b' README.md", "cargo check"]
    );
    // Trailing bracket content (`README.md:42` in a group) is preserved.
    assert_eq!(parse_bracket_list("[README.md:42], [src/lib.rs]"), vec!["README.md:42", "src/lib.rs"]);
    // A single group with a quoted comma stays one command.
    assert_eq!(parse_bracket_list("[sed -n '1,40p' README.md]"), vec!["sed -n '1,40p' README.md"]);
    assert!(parse_bracket_list("[]").is_empty());
}
