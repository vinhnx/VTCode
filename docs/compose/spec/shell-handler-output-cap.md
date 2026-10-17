---
feature: shell-handler-output-cap
status: delivered
updated: 2026-09-12
branch: fix/tools-shell-handler-output-cap
commits: ea36ec7b0..c08c617fc
---

# Shell Handler Output Cap

## Report

**What was built** — `ShellHandler` now bounds the model-visible preview of every shell command. Optional
`max_output_tokens` is parsed from the tool arguments (same field and 1–50_000 bounds as `vtcode_utility_tool_specs`),
defaulted to `DEFAULT_MAX_OUTPUT_TOKENS` (10_000). The combined stdout/stderr content is converted to a byte budget via
`OUTPUT_PREVIEW_CHARS_PER_TOKEN` (4 chars/token) and, when oversized, condensed with
`vtcode_commons::preview::condense_text_bytes` to a head/tail excerpt marked `… [N bytes omitted] …`. The `shell` tool
schema advertises `max_output_tokens` via `with_max_output_tokens_parameter`. Non-oversized output, stderr tagging, and
exit-code markers are byte-identical to prior behavior. `ThreadEvent` is untouched.

### Verification

<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode-core -E 'test(shell_handler) or test(format_shell_output) or test(resolve_max_output_tokens) or test(shell_tool_schema)'`
  — PASS (11 tests)
- `RUSTFLAGS="-D warnings" cargo clippy -p vtcode-core --lib -- -D warnings` — PASS
- `cargo check --locked -p vtcode-core --lib` — PASS
- `./scripts/check-dev.sh` — PASS (after `cargo fmt --all`)
- Independent review (general-1) against `c08c617fc` — no CRITICAL/HIGH; spec compliance, correctness, consistency all
  clean

### Journey log

- `exec_command` already had spooling + preview budgets; `ShellHandler` was a parallel path that never wired them — the
  surgical fix is preview-budget truncation, not full spooler port (S3).
- Spec originally asked for a literal `truncated: true` field; delivered marker is `… [N bytes omitted] …` from
  `condense_text_bytes`. Spec updated to match delivered behavior (reviewer MEDIUM).
- `cargo fmt --all` after the first commit required an amend; formatting-only amend did not invalidate the semantic
  review.
- Shared schema description from `with_max_output_tokens_parameter` mentions spooling; `ShellHandler` does not spool
  (S3, reviewer LOW). Acceptable for this scope.
- Four parallel explore scans (agent-loop / tools / prompts / safety) produced the top-5 table; #1 was chosen for
  surgicality + single-call token impact.

## [S1] Problem

`ShellHandler` buffers the full stdout/stderr of every shell command and returns it concatenated as model-visible
content with no byte or token cap. Unlike the `exec_command` registry path (which applies `max_output_tokens` preview
budgets, spooling, and head/tail excerpts), a single `cargo build`, `find /`, or test log can dump multi-megabyte text
into the conversation context. This is the largest single-call token waste in the tools surface.

Evidence: `crates/codegen/vtcode-core/src/tools/handlers/shell_handler.rs:113-192` — `command.output()` →
`String::from_utf8_lossy` → concatenated `content_text` with zero truncation.

## [S2] Design

Reuse the existing shared budget semantics already used by `exec_command`:

1. Parse an optional `max_output_tokens` integer from the shell tool arguments (same field name and bounds as
   `vtcode_utility_tool_specs::MAX_OUTPUT_TOKENS_FIELD`).
2. Default to `DEFAULT_MAX_OUTPUT_TOKENS` (10_000) when omitted.
3. Convert to a byte budget via `OUTPUT_PREVIEW_CHARS_PER_TOKEN` (4 chars/token).
4. If the combined stdout+stderr content exceeds the budget, condense with
   `vtcode_commons::preview::condense_text_bytes` (head + tail). The helper inserts `… [N bytes omitted] …` as the
   machine-readable truncation marker.
5. Advertise `max_output_tokens` in the `shell` tool schema via `with_max_output_tokens_parameter`.
6. Keep exit-code and stderr-tagging behavior unchanged; only the model-visible preview is bounded.

Contracts:

- Non-oversized output is byte-identical to today.
- Oversized output keeps a head and tail excerpt plus `… [N bytes omitted] …` from `condense_text_bytes`.
- The truncation marker is the model-visible signal to re-read via `exec_command` spool or a narrower command.
- `ThreadEvent` contract is untouched; this change only affects the tool result string.

## [S3] Out of Scope

- Wiring the full `exec_command` spooler into `ShellHandler` (larger refactor; preview budget is sufficient).
- Changing `exec_command` defaults or spooling thresholds.
- Safety/gateway changes.

## Tasks

- [x] T1: Add `max_output_tokens` parsing + byte-budget truncation in `ShellHandler::handle` — acceptance: oversized
      stdout/stderr is condensed; small output unchanged (covers: S2)
- [x] T2: Advertise `max_output_tokens` on the `shell` tool schema — acceptance: schema includes the field with default
      10_000 (covers: S2; depends: T1)
- [x] T3: Regression tests for oversized and undersized output — acceptance: tests pass under
      `cargo nextest run -p vtcode-core -E 'test(shell_handler)'` (covers: S2; depends: T1)
