use super::steps::parse_bracket_list;
use super::verification::{is_independent_rederivation, validate_concrete_verification};
use super::{split_bracket_items, validate_plan_content};

#[test]
fn concrete_file_existence_checks_are_valid_plan_verifiers() {
    for command in [
        "test -f docs/README.md",
        "test -d docs",
        "test -e README.md",
        "test -s 'some file.md'",
    ] {
        assert!(validate_concrete_verification(command).is_ok(), "{command}");
    }
    for command in [
        "test -f",
        "test -f $FILE",
        "test -f README.md; true",
        "test -f README.md || true",
        "test docs",
    ] {
        assert!(validate_concrete_verification(command).is_err(), "{command}");
    }
    let plan = "## Summary\nFix README link.\n\n## Implementation Steps\n1. Fix link -> files: [README.md] -> verify: [test -f docs/README.md, grep -n docs/README.md README.md]\n\n## Test Cases and Validation\nConfirm the target exists.\n\n## Assumptions and Defaults\nPreserve prose.";
    assert!(validate_plan_content(plan).is_ready());
    let tracker = super::generate_tracker_markdown_from_plan(plan).expect("tracker");
    assert!(tracker.contains("test -f docs/README.md"));
}

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

fn scope_review_plan(action: &str, targets: &str, verify: &str, outcome: &str) -> String {
    format!(
        "## Summary\nRefine documentation.\n\n## Implementation Steps\n1. Refine introduction -> files: [README.md] -> verify: [rg -n 'Overview' README.md]\n2. {action} -> files: [{targets}] -> verify: [{verify}] -> outcome: [{outcome}]\n\n## Test Cases and Validation\nHeading links resolve.\n\n## Assumptions and Defaults\nPreserve the captured workspace baseline."
    )
}

#[test]
fn explicit_scope_review_accepts_status_with_baseline_expectation() {
    let plan = scope_review_plan(
        "Review final scope",
        "README.md",
        "git status --porcelain",
        "only README.md differs from the captured baseline",
    );
    let report = validate_plan_content(&plan);
    assert!(report.is_ready(), "scope-review draft must validate: {report:?}");
    assert_eq!(report.scope_review_step_count, 1);
    assert_eq!(report.content_verification_step_count, 1);
    let tracker = super::generate_tracker_markdown_from_plan(&plan).expect("tracker");
    assert!(tracker.contains("  scope-review: git status --porcelain"));
    assert!(!tracker.contains("  verify: git status"));
    assert!(tracker.contains("  verify: rg -n 'Overview' README.md"));
}

#[test]
fn scope_review_requires_explicit_action_concrete_files_and_observable_expectation() {
    for (action, targets, verify, outcome) in [
        (
            "Implement parser",
            "src/parser.rs",
            "git status --porcelain",
            "only src/parser.rs differs from the captured baseline",
        ),
        ("Review scope", "files", "git status --porcelain", "only files differ from the captured baseline"),
        ("Review scope", "README.md", "git status --porcelain", "review changes"),
        (
            "Review scope",
            "README.md",
            "git status --porcelain",
            "only src/lib.rs differs from the captured baseline",
        ),
        (
            "Review scope",
            "README.md, src/lib.rs",
            "git status --porcelain",
            "only README.md differs from the captured baseline",
        ),
        (
            "Review scope",
            "README.md",
            "git status --porcelain; git reset --hard HEAD",
            "only README.md differs from the captured baseline",
        ),
        (
            "Review scope",
            "README.md",
            "git diff --output=report.txt",
            "only README.md differs from the captured baseline",
        ),
    ] {
        let report = validate_plan_content(&scope_review_plan(action, targets, verify, outcome));
        assert!(!report.is_ready(), "must reject {action}, {targets}, {verify}, {outcome}");
        assert_eq!(report.invalid_implementation_steps.len(), 1);
    }
    assert!(
        !validate_plan_content(&scope_review_plan(
            "Review scope",
            "README.md",
            "git diff --check",
            "only README.md changed",
        ))
        .is_ready(),
        "whitespace checks cannot establish changed-file scope"
    );
    for verify in [
        "git diff --name-only",
        "git diff --stat -- README.md",
        "git status --short",
    ] {
        assert!(
            validate_plan_content(&scope_review_plan(
                "Review scope",
                "README.md",
                verify,
                "only README.md differs from the captured baseline",
            ))
            .is_ready(),
            "must accept {verify}"
        );
    }
}

#[test]
fn scope_listing_does_not_verify_implementation_even_with_file_selector() {
    for verify in [
        "git diff --name-only -- src/lib.rs",
        "git diff --stat HEAD -- src/lib.rs",
    ] {
        let plan = scope_review_plan("Implement parser", "src/lib.rs", verify, "parser handles empty input");
        assert!(!validate_plan_content(&plan).is_ready(), "listing cannot establish implementation: {verify}");
    }
}

#[test]
fn repair_feedback_targets_defects_without_repeated_example_wall() {
    let plan = scope_review_plan("Review scope", "README.md", "git status --porcelain", "review changes");
    let feedback = validate_plan_content(&plan).repair_feedback();
    assert!(feedback.contains("step 2"));
    assert!(feedback.contains("observable expectation"));
    assert!(!feedback.contains("Valid examples:"));
    assert!(!feedback.contains("Rewrite every implementation step"));
    assert!(feedback.len() < 1600);
}

#[test]
fn retained_checkpoint_scope_review_accepts_explicit_readme_only_expectation() {
    // Sanitized final-step excerpt from checkpoint 1867. Retain the useful
    // draft with an explicit observable scope expectation, without requiring
    // the literal word baseline.
    let plan = scope_review_plan(
        "Review the final diff for scope discipline (README-only, no unrelated hunks) and confirm no worktree noise",
        "README.md",
        "git status --porcelain",
        "README-only, no unrelated hunks",
    );
    let report = validate_plan_content(&plan);
    assert!(report.is_ready(), "README-only expectation should validate: {report:?}");
    let repaired = plan.replace(
        "outcome: [README-only, no unrelated hunks]",
        "outcome: [only README.md differs from the captured baseline]",
    );
    assert!(validate_plan_content(&repaired).is_ready());
    assert!(!validate_plan_content("The plan is ready; approve it to continue.").is_ready());
}

#[test]
fn scope_review_accepts_no_other_tracked_edits_expectation() {
    // Session-shaped step 6 from session-vtcode-20261102T034357Z: the
    // expectation lives in the action text as "no other tracked edits",
    // which is semantically an exclusive changed-file expectation.
    for action in [
        "Confirm scope limited to README.md and expect status lists only README.md with no other tracked edits",
        "Confirm scope limited to README.md and expect status lists only README.md with no other tracked edit",
    ] {
        let plan = scope_review_plan(
            action,
            "README.md",
            "wc -l README.md, git status --short -- README.md",
            "scope check recorded",
        );
        let report = validate_plan_content(&plan);
        assert!(report.is_ready(), "tracked-edits expectation must validate ({action}): {report:?}");
        assert_eq!(report.scope_review_step_count, 1);
    }
}

#[test]
fn full_session_plan_with_tracked_edits_scope_review_is_ready() {
    // Verbatim rejected draft from session-vtcode-20261102T034357Z: the
    // validator ended the turn instead of presenting the plan, because step
    // 6 ("no other tracked edits") was not accepted as an exclusive
    // changed-file expectation. The whole artifact must validate so the
    // draft reaches the approval gate instead of a terminal rejection.
    let plan = r#"## Summary
Restructure README.md install, desktop, docs, and contribution sections while preserving uncommitted prebuilt-asset installer wording and working links.

## Scope
In: README.md hero, Overview, Quick start install/configure, Usage, Desktop app, Documentation catalog, Development, Contributing/Community/Thanks/Support, License headings and links.
Out: No code, installer scripts, desktop sources, docs content, contributor list, screenshots, badges, or sponsorship changes.

## Implementation Steps
1. Consolidate Quick start install variants into one Install flow -> files: [README.md] -> verify: [grep -n '^### 1. Install' README.md, grep -n 'NATIVE_INSTALLERS.md' README.md]
2. Merge Desktop app download, installer offer, update with-desktop, and from-source fallback into one path -> files: [README.md] -> verify: [grep -n '^## Desktop app' README.md, grep -n 'desktop-workspace.md' README.md]
3. Deduplicate Documentation catalog against Quick start and Usage links -> files: [README.md] -> verify: [grep -n '^## Documentation' README.md, grep -n 'docs/INDEX.md' README.md]
4. Consolidate Contributing, Community, Thanks contributors, and Support sections without losing conduct, templates, or sponsor note -> files: [README.md] -> verify: [grep -n '^## Contributing' README.md, grep -n 'CODE_OF_CONDUCT.md' README.md]
5. Tighten Overview and Usage wording and repair heading anchors after renames -> files: [README.md] -> verify: [grep -n '^#{1,4} ' README.md, grep -n 'planning-workflow.md' README.md]
6. Confirm scope limited to README.md and expect status lists only README.md with no other tracked edits -> files: [README.md] -> verify: [wc -l README.md, git status --short -- README.md]

## Test Cases and Validation
- Single Install, Desktop, Documentation, and Contributing headings remain: grep -n '^#{1,4} ' README.md
- Desktop and installer references resolve: grep -n 'desktop-workspace.md' README.md, grep -n 'NATIVE_INSTALLERS.md' README.md
- Preserved uncommitted installer wording visible: git diff -- README.md
- Length stays under 1000-line ceiling: wc -l README.md
- Markdown lint if available, otherwise report skipped: npx markdownlint-cli2 README.md

## Assumptions and Defaults
- Preserve existing uncommitted README.md installer and desktop asset wording in meaning.
- Restructure means merge and retitle, not delete coverage for install methods, desktop signing, docs entry points, or contribution channels.
- README.md only; linked docs files are read-only references.

## Expected Outcomes
- Shorter README.md with one Install path, one Desktop path, one docs catalog, and one contribution block.
- All retained headings, anchors, and docs paths resolve with user edits intact.

## Dependencies and Prerequisites
- No new tooling; Markdown lint optional and skippable if unavailable.
"#;
    let report = validate_plan_content(plan);
    assert!(report.is_ready(), "session draft must validate: {report:?}");
    assert_eq!(report.implementation_step_count, 6);
    assert_eq!(report.scope_review_step_count, 1);
    // Asymmetric boundary: dropping the changed-file wording must fail on
    // exactly step 6, proving the expectation check bites rather than the
    // test passing because scope-review detection was skipped.
    let without_expectation = plan.replace(" with no other tracked edits", "");
    let report = validate_plan_content(&without_expectation);
    assert!(!report.is_ready(), "scope review without a changed-file expectation must stay invalid");
    assert_eq!(report.invalid_implementation_steps.len(), 1);
    assert!(
        report.invalid_implementation_steps[0].starts_with("step 6:"),
        "only the scope-review step must fail: {:?}",
        report.invalid_implementation_steps
    );
}

#[test]
fn multiline_scope_review_preserves_separate_metadata_in_tracker() {
    let plan = scope_review_plan(
        "Review scope",
        "README.md",
        "git status --porcelain",
        "only README.md differs from the captured baseline",
    )
    .replace(
        "2. Review scope -> files: [README.md] -> verify: [git status --porcelain] -> outcome:",
        "2. Review scope\n   files: [README.md]\n   verify: [git status --porcelain]\n   outcome:",
    );
    assert!(validate_plan_content(&plan).is_ready());
    let tracker = super::generate_tracker_markdown_from_plan(&plan).expect("tracker");
    assert!(tracker.contains("  scope-review: git status --porcelain"));
    assert!(tracker.contains("  outcome: only README.md differs from the captured baseline"));
}

#[test]
fn scoped_action_detection_requires_a_review_verb() {
    for action in [
        "Confirm scoped change shows only README.md modified",
        "Review scoped diff for worktree noise",
        "Check scoped files against the captured baseline",
        "Verify scoped edits left no unrelated hunks",
    ] {
        assert!(super::verification::is_scope_review_action(action), "{action}");
    }
    for action in [
        "Scoped refactor of parser",
        "Implement scoped parser change",
        "Implement parser",
        "Review final diff",
    ] {
        assert!(!super::verification::is_scope_review_action(action), "{action}");
    }
}

#[test]
fn scope_review_accepts_gate_paired_with_git_scope_check() {
    // Session-shaped step 5 from session-vtcode-20261101T022006Z (turns
    // 1939-1940): a scope gate pairs a concrete gate command with a Git
    // scope check instead of using the Git check alone.
    for action in [
        "Confirm scoped change shows only README.md modified plus file-length gate passes",
        "Run file-length gate and confirm scoped change lists only README.md modified",
        "Run README-only gates and confirm no scope creep",
    ] {
        for verify in [
            "python3 scripts/check_file_length.py --mode enforce, git status --porcelain README.md",
            "python3 scripts/check_file_length.py --mode enforce, git diff --stat HEAD -- README.md",
            "[python3 scripts/check_file_length.py --mode enforce] and [git status --porcelain README.md]",
        ] {
            let plan = scope_review_plan(action, "README.md", verify, "only README.md modified");
            let report = validate_plan_content(&plan);
            assert!(report.is_ready(), "mixed scope step must validate ({action} / {verify}): {report:?}");
            assert_eq!(report.scope_review_step_count, 1);
        }
    }
    let plan = scope_review_plan(
        "Confirm scoped change shows only README.md modified plus file-length gate passes",
        "README.md",
        "python3 scripts/check_file_length.py --mode enforce, git status --porcelain README.md",
        "only README.md modified",
    );
    let tracker = super::generate_tracker_markdown_from_plan(&plan).expect("tracker");
    assert!(tracker.contains("  verify: python3 scripts/check_file_length.py --mode enforce"));
    assert!(tracker.contains("  scope-review: git status --porcelain README.md"));
    assert!(!tracker.contains("  scope-review: python3"));
}

#[test]
fn scope_review_rejects_gate_without_scope_check_or_vague_companion() {
    for verify in [
        "python3 scripts/check_file_length.py --mode enforce",
        "git status --porcelain README.md, run checks",
        "run checks, git diff --stat HEAD -- README.md",
    ] {
        let plan = scope_review_plan(
            "Confirm scoped change shows only README.md modified plus file-length gate passes",
            "README.md",
            verify,
            "only README.md modified",
        );
        let report = validate_plan_content(&plan);
        assert!(!report.is_ready(), "must reject {verify}");
        assert_eq!(report.invalid_implementation_steps.len(), 1);
    }
    let feedback = validate_plan_content(&scope_review_plan(
        "Confirm scoped change shows only README.md modified plus file-length gate passes",
        "README.md",
        "git status --porcelain README.md, run checks",
        "only README.md modified",
    ))
    .repair_feedback();
    assert!(feedback.contains("verification item 2 must be a concrete command or check"), "{feedback}");
}

#[test]
fn sanitized_checkpoint_1940_retry_plan_is_ready() {
    // Full retry-turn plan from session-vtcode-20261101T022006Z turn 1940:
    // concrete `test -f` verifiers plus a mixed scoped gate step.
    let plan = include_str!("fixtures/checkpoint-1940-plan.txt");
    let report = validate_plan_content(plan);
    assert!(report.is_ready(), "retained retry plan should validate: {report:?}");
    let unwrapped = plan.replace("<proposed_plan>", "").replace("</proposed_plan>", "");
    assert!(validate_plan_content(&unwrapped).is_ready(), "complete unwrapped draft remains valid");
    assert_eq!(report.implementation_step_count, 5);
    assert_eq!(report.scope_review_step_count, 1);
    assert_eq!(report.content_verification_step_count, 4);
    let tracker = super::generate_tracker_markdown_from_plan(plan).expect("tracker");
    assert!(tracker.contains("  scope-review: git status --porcelain README.md"));
    assert!(tracker.contains("  verify: python3 scripts/check_file_length.py --mode enforce"));
    assert!(tracker.contains("test -f docs/README.md"));
}

#[test]
fn sanitized_checkpoint_1867_retains_a_complete_approvable_draft() {
    let plan = include_str!("fixtures/checkpoint-1867-plan.txt");
    let report = validate_plan_content(plan);
    assert!(report.is_ready(), "retained checkpoint plan should validate: {report:?}");
    let unwrapped = plan.replace("<proposed_plan>", "").replace("</proposed_plan>", "");
    assert!(validate_plan_content(&unwrapped).is_ready(), "complete unwrapped draft remains valid");
    assert_eq!(report.implementation_step_count, 10);
    assert_eq!(report.scope_review_step_count, 1);
    assert_eq!(report.content_verification_step_count, 9);
    let tracker = super::generate_tracker_markdown_from_plan(plan).expect("tracker");
    assert!(tracker.contains("  scope-review: git status --porcelain\n"));
    assert!(tracker.contains("  scope-review: git diff --stat HEAD -- README.md"));
    assert!(!tracker.contains("  verify: git status"));
    assert!(!tracker.contains("  verify: git diff --stat"));
}
