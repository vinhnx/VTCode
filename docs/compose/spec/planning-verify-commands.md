---
feature: planning-verify-commands
status: delivered
updated: 2026-09-19
branch: fix/planning-verify-review
commits: 9c2ac1f8c..HEAD
---

# Planning verify commands accept common inspection tools

## Report

**What was built** — Post-merge review of the planning-verify merge found a Medium validation regression: expanding
`COMMAND_NAMES` with English-word binaries let prose verifies such as `file changes` / `sort order` pass the
command-head rule (multi-token only). The amendment keeps real inspection verifies valid while requiring command
evidence for ambiguous heads.

1. **Ambiguous command heads** (`artifacts.rs`): heads in `AMBIGUOUS_COMMAND_HEADS` (`cat`, `cut`, `diff`, `file`,
   `find`, `head`, `just`, `ls`, `make`, `sort`, `stat`, `tr`, `uniq`, `wc`) now need a later flag (`-n`, `--locked`) or
   filename/path-like token (`src/file.rs`, `README.md`, `./scripts/…`). Unambiguous heads (`cargo`, `rg`, `sed`,
   `grep`) keep the multi-token rule so `cargo test` stays valid. `git` remains absent; `git diff --check` stays
   rejected. Flag-shaped tokens keep their leading hyphen in `verification_words` so `-l` is not stripped to prose.
2. **Shared example constants**: `PLANNING_VERIFY_VALID_EXAMPLES` / `PLANNING_VERIFY_INVALID_EXAMPLES` in
   `planning_workflow::artifacts`, embedded by `repair_feedback()` and `build_plan_repair_directive()`. Binary
   recovery/repair prompt constants keep compile-time literals; module-local presence tests assert inspection examples
   and `git diff --check`.
3. **Filtered false positives**: `cargo test` without flags; `git diff --check` still invalid; `COMMAND_NAMES` is
   plan-validation only (not shell safety); pre-existing UI modal commit and uncommitted main edits out of scope.
4. **Docs**: quality line + planning-workflow.md + SESSION_LOG_REVIEW note the ambiguous-head rule.

**Verification** — commands run and observed results:

<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode-core -E 'test(inspection) or test(english_phrase) or test(validate_plan) or test(repair_feedback) or test(plan_quality)'`
  — PASS (55)
- `cargo nextest run -p vtcode -E 'test(repair_directive) or test(quality_line) or test(planning_recovery_prompt) or test(missing_plan)'`
  — PASS (9)
- `cargo check --locked -p vtcode-core -p vtcode` — PASS
- `cargo fmt --all -- --check` — PASS
- Independent re-review (2026-09-19): AC1–AC5 met; no critical findings. Residual `wc README.md`/`head README.md`
  slash-less filename false-reject closed in the delivery follow-up.

**Journey log** — at most 5 entries that help future work:

1. Expanding a command-head allowlist with English words requires a second token of command evidence; multi-token length
   alone is insufficient.
2. Keep `git` out of plan-verify `COMMAND_NAMES` or `git diff --check` re-approves as a multi-token head.
3. Tokenizers that strip ASCII punctuation must preserve leading hyphens on flag-shaped tokens or flag-evidence gates
   fail closed (`wc -l`).
4. Compiled prompt constants cannot `concat!`; shared fragments + substring presence tests are the maintainable
   multi-surface pattern.
5. Ambiguous-head path evidence should accept slash-less filenames (`README.md`), not only `/` or `./` shapes.

## [S1] Problem

Planning recovery rejected valid inspection `verify:` items (`sed`, `grep`). Fixed in merge `4aa01687f..c2872921a`.

Post-merge review confirmed an unintended validation regression: `COMMAND_NAMES` now includes common English words
(`file`, `sort`, `find`, `ls`, `cat`, `cut`, `tr`, `make`, …). The command-head rule only required `words.len() > 1`, so
prose verifies such as `file changes` or `sort order` now validate as concrete commands and can approve plans without a
real check.

Secondary: valid/invalid verify examples are duplicated across many compiled prompt constants with no shared source and
no sweep test, so residual surfaces can drift (observed once already).

## [S2] Design

### Ambiguous command heads

Keep the expanded inspection allowlist (real `sed -n` / `grep -n` / `head -40` verifies must stay valid).

Treat these heads as **ambiguous English words**: `cat`, `cut`, `diff`, `file`, `find`, `head`, `just`, `ls`, `make`,
`sort`, `stat`, `tr`, `uniq`, `wc`.

For a verify whose command head is ambiguous, require at least one later token that looks like command evidence:

- a flag (`-n`, `-l`, `--locked`, …), or
- a path-like token (`src/file.rs`, `./scripts/check-dev.sh`, absolute path).

Unambiguous heads (`cargo`, `rg`, `sed`, `grep`, `python3`, `npm`, …) keep the existing multi-token rule so `cargo test`
and `grep -n …` stay valid without a path.

`git` remains absent from `COMMAND_NAMES`; `git diff --check` stays invalid.

`is_actual_command_token` / `COMMAND_NAMES` remain plan-validation only — not used by shell safety / readonly
classification.

### Example-list DRY guard

Publish shared example fragments in `planning_workflow::artifacts`:

- `PLANNING_VERIFY_VALID_EXAMPLES`
- `PLANNING_VERIFY_INVALID_EXAMPLES`

Use them from `PlanValidationReport::repair_feedback()`. Binary runloop constants keep compile-time string literals
(Rust cannot `concat!` consts), but a presence test must assert every shipped planning synthesis/repair surface contains
the inspection valid examples (`sed -n`, `grep -n`) and the invalid examples (`run checks`, `git diff --check`).

## [S3] Out of Scope

- Loosening or rewriting observational / independent-rederivation verify paths.
- Changing recovery thresholds, budgets, or blocked-handoff policy.
- Reviewing unrelated pre-existing main commits (UI plan-modal work) or uncommitted local edits.
- Free-form “looks like shell” heuristics beyond the ambiguous-head rule.

## Tasks

- [x] T1: Reject English-phrase command-head verifies — acceptance: `file changes`, `sort order`, `find files`,
      `make sense` fail `validate_concrete_verification`; `sed -n '81,88p' README.md`, `grep -n 'symbol' src/file.rs`,
      `head -40 docs/file.md`, `wc -l README.md`, `file src/main.rs`, `cargo test` still pass. (covers: S2)
- [x] T2: Shared verify-example constants + repair_feedback wiring — acceptance: `repair_feedback()` includes
      `PLANNING_VERIFY_VALID_EXAMPLES` content; constants exported from artifacts. (covers: S2; depends: T1)
- [x] T3: Prompt-surface presence tests — acceptance: module-local presence tests cover quality line, repair_feedback,
      synthesis hint, turn_loop recovery constants, response_handling directives, exit_trigger missing-plan synthesis,
      and empty-response recovery examples. (covers: S2; depends: T2)
- [x] T4: Feature-doc amendment + session log residual — acceptance: this doc records the review amendment;
      `SESSION_LOG_REVIEW.md` notes the ambiguous-head tightening. (covers: S2; depends: T1)
