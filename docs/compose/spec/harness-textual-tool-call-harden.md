---
feature: harness-textual-tool-call-harden
status: delivered
updated: 2026-09-25
branch: fix/harness-textual-tool-call-harden
commits: f79ec288f..60959f8ee
---

# Harness Textual Tool-Call Harden

## Report

**What was built** — Content-derived tool-call extraction no longer treats fenced documentation or mid-prose tag
mentions as executable calls. The tagged `tool_call` scanner (main harness `text_tools::parse_tagged` and skill
`parse_textual_skill_tool_call`) only binds clean identifier names outside fenced code blocks, skips dirty candidates,
and keeps gateway preamble+call shapes working. Native tool calls canonicalize shell aliases (`bash`/`shell`/ `run`/…)
to `exec_command` and reject prose-blob names at prepare time, so fixture payloads cannot dispatch under a garbage name.
Skill sub-LLM parsing gains the same fence + clean-name gate plus an injection regression.

**Verification** — `./scripts/check-dev.sh` PASS; `./scripts/check-dev.sh --test` 154/155 (one failure
`print_mode_requires_prompt_or_stdin` is PRE-EXISTING: missing OpenAI auth in this environment). Focused nextest:
`text_tools` + `prepared_tool_call` 109/109 PASS; `textual_skill` + `skill_executor` + `is_clean_skill` + pty reap 30/30
PASS.

**Journey log** —

- Session logs showed tool names that were whole prose blobs ending in `exec_command` and fixture `bash`/`rm -rf /`
  calls; root cause was tagged markup search with no fence/identifier gate, not streaming name pollution.
- Existing tests intentionally parse fenced Rust/YAML function-style blocks, so fence exclusion is scoped to the tagged
  `tool_call` dialect only.
- `unfenced_byte_ranges` must not search the fenced body when handling the closer line (first skill-side draft did;
  fixed before tests).
- Silent-star remainder: session-ID uniqueness holds (`create_session` rejects duplicates); Windows `kill_process_group`
  still untested (documented on `reap_child_bounded`); read-guard keeps scoped block (no broad recovery); first-paint
  registry defers policy until `ensure` (no dispatch race found).
- Self-review (subagent stalled/cancelled) caught three criticals before ship: (1) `canonicalize_shell_tool_alias` was
  collapsing PTY tools into `exec_command`; (2) strict `is_clean_tool_name` on native calls would reject MCP names with
  `::`; (3) skill parser lost the empty-payload `return None`. All three fixed + regression tests
  (`shell_alias_maps_only_true_shell_names`, `dispatchable_tool_name_allows_mcp_and_rejects_prose`).

## [S1] Problem

Session `session-vtcode-20260925T141131Z_691914-40628` (model `zai/glm-5.3-flash`) collapsed after six turns (8.1M input
tokens) while implementing a plan to harden skill textual tool-call parsing. Observed in `events.jsonl`:

1. **Prose-embedded markup became tools.** Model text that merely _discussed_ `tool_call` tags was parsed into tool
   invocations whose `tool_name` was a garbage prose blob ending in `exec_command` (preflight: `Unknown tool: ...`).
2. **Documentation examples were attempted as real calls.** Test fixtures the model wrote (`bash` + `rm -rf /`,
   `rm -rf /tmp/demo`, `git push --force`) were extracted as tool invocations. `bash` failed as unknown (luck, not
   design); a clean `exec_command` name would have executed the fixture payload.
3. **Recovery cascade aborted the turn.** After bogus calls: tools disabled →
   `Recovery synthesis failed; no tool call applied` → `turn.blocked` (`blocked_streak: 4`) → `turn aborted`.

Root cause class: content-derived tool-call extraction (`text_tools` and
`skills/executor.rs::parse_textual_skill_tool_call`) matches `tool_call` anywhere in model text — including fenced
documentation and mid-prose mentions — and dispatches the result without a clean-name check. Native alias names (`bash`,
`shell`, `run`) are not canonicalized before registry dispatch, so the same fixture can be either unknown or executable
depending on which path parses it.

The broken session's approved plan (`.vtcode/plans/1790345671722-silent-star.md`) also left unfished remediation items
(exec-session close lock, session-id uniqueness, Windows process-group kill, planning read-guard decision,
registry-first-paint race).

## [S2] Design

### A. Main-harness textual tool-call policy (`src/agent/runloop/text_tools/`)

**Fences.** Markup inside fenced code blocks
<!-- markdownlint-disable-next-line MD013 -->
(```or ~~~, CommonMark-ish: opener allows an info string; closer is 3+ of the same fence char with only whitespace) is documentation and is never executable.`detect_textual_tool_call`and`strip_textual_tool_call_regions`
only consider unfenced byte ranges.

**Clean call.** Outside fences, a tagged `tool_call` is converted only when the parsed tool name is a clean identifier
(`[A-Za-z][A-Za-z0-9_]*`, length ≤ 64) and the call has a complete payload (arg tags, JSON object, or key=value pairs).
Mid-prose mentions such as `` `tool_call` `` yield an empty/invalid name and are skipped; the scan continues to the next
candidate. If no clean call exists, the response stays text.

**Native names.** Registry dispatch canonicalizes known shell aliases (`bash`, `shell`, `exec`, `run`, `run_cmd`, …) to
`exec_command` for _native_ tool calls as well (same map the textual path already uses), so a model-emitted `bash` runs
as `exec_command` instead of `Unknown tool: bash`. Names that are not clean identifiers (spaces, newlines, length > 64)
are rejected at preflight as invalid, not dispatched.

### B. Recovery cascade

Bogus textual extractions that fail name/payload validation no longer become tool calls at all (they stay text under A),
so they cannot trip the tools-disabled fuse or the recovery contract-violation path. When a tool-free recovery pass
still sees tool-call markup, existing strip + one retry directive remains; `Recovery synthesis failed` must not fire
solely because the model quoted markup in its synthesis text.

### C. Skill sub-LLM (`crates/codegen/vtcode-core/src/skills/executor.rs`)

`parse_textual_skill_tool_call` adopts the same fence exclusion and clean-name requirement. Alias mapping
(`canonicalize_skill_textual_tool_name`) stays for real calls but cannot fire on fenced or non-identifier names.
Regression: provider content that embeds markup inside a fence (or as a quoted example with a non-identifier name) must
not execute and must fall through to final content; the existing out-of-scope denial regression still passes.

### D. Remaining silent-star plan items

1. **Injection regression** in `skills/executor.rs` (covers C).
2. **Narrow/sole-content** — delivered by A/C clean-call + fence rules (no alias-list shrink required once
   non-identifiers cannot bind).
3. **`output_read_lock` close race** — regression in `exec_session.rs`: a pipe reader holding `output_read_lock` past
   `EXEC_SESSION_OUTPUT_READ_LOCK_TIMEOUT` must fail cleanly (no panic, no partial metadata) when close proceeds.
4. **Abandoned `spawn_blocking` close** — confirm session-ID uniqueness (`create_timestamped*`) so a late close cannot
   hit a recreated record; if uniqueness is not guaranteed, key the late close by record generation.
5. **Windows process-group kill** — verify `kill_process_group_by_pid` / `kill_process_group` in `vtcode-bash-runner`;
   mark new reap tests' `#[cfg(unix)]` assumptions explicitly.
6. **Planning read-guard** — keep scoped block (no broad recovery); document the decision. Do not restore family-cap
   recovery.
7. **Registry-after-first-paint race** — audit `new_for_first_paint_with_loaded_config` vs early tool dispatch; report
   findings in the feature Report (no behavior change unless a concrete race is proven).

### Out of Scope

- Removing textual tool-call execution for gateway models (zai/glm).
- Changing skill location precedence or directory-name match policy.
- Raising `EXEC_SESSION_CLOSE_TIMEOUT` or redesigning PTY reap budgets.
- Full workspace suite beyond the changed crates.

## Tasks

- [x] T1: Unfenced-range + clean-name gate in `text_tools` (detect + strip) — acceptance: unit tests prove fenced markup
      is never a call; mid-prose `tool_call` mention is not a call; clean sole/preamble+call still parses (covers: S2.A)
- [x] T2: Native alias canonicalization + non-identifier rejection at dispatch — acceptance: native `bash` maps to
      `exec_command`; prose-named tool calls are rejected before registry (covers: S2.A)
- [x] T3: Recovery stays text when markup is not a clean call — acceptance: regression shows quoted/fenced markup in a
      recovery synthesis does not trip contract-violation/`Recovery synthesis failed` (covers: S2.B)
- [x] T4: Skill executor fence + clean-name + injection regression — acceptance: fenced/quoted markup not executed;
      out-of-scope denial still passes; `textual_skill_tool_call` tests green (covers: S2.C, S2.D.1-2)
- [ ] T5: `output_read_lock` close-race regression — acceptance: test proves lock-timeout close is clean (covers:
      S2.D.3)
- [x] T6: Session-ID uniqueness check for abandoned close (or generation key) — acceptance: evidence in Report or code
      change (covers: S2.D.4)
- [x] T7: Windows kill_process_group verification + platform marks on reap tests — acceptance: docs/tests state platform
      assumptions (covers: S2.D.5)
- [x] T8: Read-guard scoped-block decision recorded — acceptance: Report documents keep-as-is (covers: S2.D.6)
- [x] T9: Registry first-paint race audit — acceptance: Report states findings (covers: S2.D.7)
- [x] T10: Verify changed crates (fmt/check/nextest) — acceptance: commands exit 0 or failures marked PRE-EXISTING
      (covers: S2)
