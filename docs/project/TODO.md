implement control+r to search all prompts with actual logic, reference.

/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/search_prompts.png

---

implement double escape to rewind, reference.

1. show rewind history, reference. '/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-06-07 at 9.15.02 AM.png'
2. when user select a history. show 4 options. implement actual logic you see from the screenshot. reference. ''/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-06-07 at 9.15.35 AM.png'

---

add this to vtcode's system prompt and design with the idea:

```
## Structural Code: ast-grep
Reach for 'ast-grep" whenever the work is structural - finding, matching, or rewriting code by its shape rather than its text. ast-grep matches the AST through tree-
sitter, so it sees real call expressions, declarations, and JSX nodes where grep only sees characters, and it speaks TypeScript, Python, Rust, Go, and more - one tool across the whole stack. It is installed on every machine (ast-grep -version).
Use it in two modes:
- **Search and codemod.** Before editing many files by hand, write one ast-grep pattern. 'SVAR' matches a single node, '$$$ARGS' matches many. A migration becomes deterministic, auditable, reversible pass instead of forty edits that drift and miss sites.
- find: 'ast-grep -p 'foo($$$ARGS)' -1 ts*
- rewrite: 'ast-grep -p '$A && $A()' -1 ts -r '$A?. () ' (inspect the diff, then add '-U' to apply)
- **Lint gate.** When a repo carries 'sgconfig.yml"
+ a rules/ dir, run ast-grep scan; it enforces the project's banned patterns and exits non-zero on any error-
severity match. When 'ast-grep scan" belongs to the repo's gate, it passes before you commit.
Prefer 'ast-grep' over grep/ripgrep for any code-shape question; keep text grep for prose, logs, and config strings. Always invoke the 'ast-grep' command - the "sg' alias collides with the system
sg •
```

check if vtcode's should have support for ast-grep built in, or just recommend it as a tool to use alongside vtcode. if built in, implement a command like '/astgrep' that takes the same arguments as the CLI and runs it, showing results in the TUI.

---

---

# VT Code Agent Harness Robustness Plan

Source logs reviewed:

- `.vtcode/checkpoints/turn_326.json`
- `.vtcode/checkpoints/turn_327.json`
- `.vtcode/checkpoints/turn_333.json`
- `.vtcode/checkpoints/turn_334.json`
- `.vtcode/checkpoints/turn_343.json` through `.vtcode/checkpoints/turn_350.json`

## Counting method

Important: checkpoint files are cumulative conversation snapshots. Do **not** sum every error found in every file; that double-counts earlier failures repeated in later checkpoints.

Use both views:

- **Turn-delta view**: count only tool errors after each checkpoint's `metadata.prompt_message_index`.
- **Unique view**: deduplicate by `tool_call_id` plus error category/message.

The first rough pass over-counted repeated historical errors. The corrected turn-delta/unique analysis is below.

## Corrected ranked findings

| Rank | Level | Category                                         | Corrected evidence                                                                                                                                                                                                                                                            | Impact                                                                                                                             | Proposed fix                                                                                                                                                                                                                                                                             |
| ---- | ----- | ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1    | P1    | Recovery-mode prose instead of executable action | Turns 347, 349, and 350 end with large README content or pseudo tool-call markup in assistant text instead of an executed patch/write. This is visible in final assistant messages such as “Since tools are disabled in this recovery pass...” and inline `<tool_call>` text. | Highest user-visible failure: the agent appears to continue work but stops at instructions/content for the user to apply manually. | Document as a harness completion invariant: if a mutating task is requested and tools are unavailable/denied, final output must explicitly say “not applied” and list the blocker, not present generated content as completion. Future implementation: recovery-mode finalization check. |
| 2    | P1    | Structural-search misuse                         | Corrected delta: 2 new failures in turn 343. Unique across snapshots: 4 structural preflight failures. Examples: `AgentLoop                                                                                                                                                   | agent_loop                                                                                                                         | agent loop`, `fn run                                                                                                                                                                                                                                                                     | fn execute | fn tick`, and selector-bearing searches with non-parseable `pattern`. | Wastes early turn budget and creates noisy recovery loops before useful work starts. | Clarify docs/tool guidance: use grep/text search for prose, regex, symbol names, and alternatives; use structural mode only when the pattern is parseable code. Add checkpoint metric for structural-preflight failures. |
| 3    | P1    | Edit primitive mismatch                          | Corrected delta: 1 new failure in turn 343. Unique: 1. `unified_file` edit attempted a large README block replacement beyond the small literal-edit limit.                                                                                                                    | Blocks simple documentation changes and can push the agent into fallback loops.                                                    | Clarify docs/tool guidance: `unified_file` edit is for small literal replacements; larger edits should use patch-style edits with exact context or intentional overwrite. Add examples to tool docs.                                                                                     |
| 4    | P1    | Brittle patch context                            | Corrected delta: 1 new failure in turn 344. Unique: 1. Patch used synthetic anchor `@@ Real-world workflows @@` rather than exact file context.                                                                                                                               | Prevents forward progress after partial edits and increases repeated reads.                                                        | Document patch discipline: read a narrow fresh range, copy exact surrounding context, and re-read when context drifts. Add evaluator category for patch-context failures.                                                                                                                |
| 5    | P2    | Policy-denied shell fallback                     | Corrected delta: 2 new failures, in turns 348 and 349. Unique: 2. `unified_exec` was denied for shell-style README inspection (`grep ... README.md`, then `cat README.md`).                                                                                                   | Wastes turns after policy already gave a hard answer and contributes to recovery-mode prose.                                       | Document behavior: after policy denial, switch to read/search tools or ask the user; do not retry shell inspection. Future implementation: same-turn denial cache/hint.                                                                                                                  |
| 6    | P2    | Create-vs-overwrite mismatch                     | Corrected delta: 2 new failures in turns 326 and 334. Unique: 2. Existing `README.md` and `docs/diagrams/agent-loop-sequence.md` writes omitted intentional overwrite mode.                                                                                                   | Common when revising generated docs; recovery was possible but cost extra turns.                                                   | Improve examples: existing-file writes require `mode=overwrite`; otherwise use edit/patch primitives.                                                                                                                                                                                    |
| 7    | P2    | Malformed or legacy tool calls                   | Corrected delta: 3 new failures: 1 malformed XML-style `unified_file` call in turn 333 and 2 legacy `exec` calls in turn 334. Unique: 3.                                                                                                                                      | Tool preflight rejects the call before any useful work happens.                                                                    | Keep tool names explicit in docs and prompts; add recovery hint for unknown legacy aliases like `exec`/`bash`.                                                                                                                                                                           |
| 8    | P3    | Repeated read loop                               | Corrected delta: 1 new guard trip in turn 349. Unique: 1. Repeated full README reads were blocked with “Reuse the collected output...” guidance.                                                                                                                              | Existing guard works, but the agent should switch tactics earlier.                                                                 | Keep the guard. Add docs guidance to narrow ranges, summarize collected evidence, or re-plan instead of re-reading the same file.                                                                                                                                                        |

## Turn-delta summary

| Turn | User prompt               | New failure categories after `prompt_message_index`                                | Outcome note                                                            |
| ---- | ------------------------- | ---------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| 326  | `revamp the readme`       | create-vs-overwrite: 1                                                             | Recovered with overwrite and completed.                                 |
| 327  | slim README agent guide   | none new                                                                           | Completed. Earlier turn history is present but should not be recounted. |
| 333  | diagram render error      | malformed tool call: 1                                                             | Created/fixed diagram but emitted malformed tool XML once.              |
| 334  | diagram render error      | legacy tool alias: 2; create-vs-overwrite: 1                                       | Recovered with correct tool/overwrite and completed.                    |
| 343  | README real use case      | structural-search misuse: 2; edit primitive mismatch: 1                            | Completed after fallback.                                               |
| 344  | TUI use cases             | brittle patch context: 1                                                           | Completed after re-reading exact context.                               |
| 345  | autonomous capability     | none new                                                                           | Completed.                                                              |
| 346  | sponsor section           | none new                                                                           | Completed.                                                              |
| 347  | structure/content cleanup | no tool error, but recovery-mode prose/pseudo-apply output                         | User-visible incomplete application risk.                               |
| 348  | image border radius       | policy-denied exec: 1                                                              | Completed despite denied shell grep.                                    |
| 349  | continue                  | policy-denied exec: 1; repeated-read guard: 1; recovery-mode pseudo tool-call text | Did not actually apply the full README rewrite.                         |
| 350  | do it                     | no new tool error, but final says tools are disabled and provides content to apply | Confirms recovery-mode completion failure.                              |

## Deep-dive root cause notes

### Recovery-mode prose completion is the highest-risk bug

The logs show an interactive turn recovery path where tools become unavailable or intentionally disabled for a tool-free synthesis pass. In that state, the model produced text that still contained pseudo tool-call markup, for example inline `<tool_call>` / `<function=apply_patch>` content. Because the pseudo call was embedded inside prose instead of emitted as a structured provider tool call, it could be treated as a final assistant answer.

Expected harness behavior:

- Do not execute tools during a tool-free recovery pass.
- Do not display pseudo tool-call markup as if it was applied.
- End with an explicit blocker: “not applied; tools were disabled/denied; latest usable evidence is above; next action is ...”.

Proposed surgical implementation point for the later code pass:

- `src/agent/runloop/unified/turn/turn_processing/result_handler.rs`
    - Extend the recovery `TurnProcessingResult::TextResponse` guard so it detects embedded pseudo tool-call intent, not only parseable full-message textual tool calls.
    - Markers to treat as recovery contract violations include `<tool_call`, `</tool_call>`, `<function=`, `<parameter=`, `<invoke name=`, and MiniMax tool-call tags.
    - Add a regression test using the exact failure shape: prose followed by `<tool_call><function=apply_patch>...`.
- `src/agent/runloop/unified/turn/turn_loop.rs`
    - Make the deterministic recovery fallback explicitly say no additional tool call was applied.

This is a guardrail, not a new execution path. The harness should prefer honesty over trying to “rescue” malformed tool markup during a no-tools recovery phase.

### Tool-selection failures are secondary but still worth documenting

The corrected counts show only a few unique tool-selection failures, but they explain how the session entered recovery pressure:

- structural search was chosen for regex/text alternatives,
- large block edits were attempted through small literal-edit primitives,
- patch context used synthetic anchors,
- shell inspection continued after policy denial.

These should be handled by examples and metrics before adding more policy logic.

## Docs-first fix plan

1. Keep this corrected incident analysis in the project TODO so future harness work starts from observed logs and avoids cumulative-checkpoint double-counting.
2. Track the issue in `docs/harness/TECH_DEBT_TRACKER.md` as a P1 Harness item, explicitly including recovery-mode prose completion.
3. Next docs pass: update tool usage docs with compact examples for:
    - checkpoint-log counting methodology (`prompt_message_index` deltas plus unique dedup),
    - `unified_search` grep vs structural mode,
    - `unified_file` edit vs overwrite,
    - patch context discipline,
    - policy-denied exec recovery,
    - recovery-mode final wording: “not applied” vs “done”.
4. Future implementation pass: add checkpoint/session evaluator metrics for the eight categories above.
5. Future implementation pass: consider hard guards for repeated identical policy-denied exec attempts, legacy tool aliases, and final responses that include pseudo tool-call markup.

## Validation for this docs pass

- No code changes required.
- Review docs diff only: `git diff -- docs/project/TODO.md docs/harness/TECH_DEBT_TRACKER.md`.

==

should we merge vtcode-design, vtcode-themes, vtcode-tui => vtcode-ui? the centralize crate for all UI components and design system. this way we can have a single source of truth for all UI related code and design decisions, and it will be easier to maintain and evolve the UI as a whole. also, it will reduce the number of crates we have to manage and publish, and it will make it easier for contributors to find and work on UI related code.

===

fix auto mode denied policy for running bash command. maybe suggest user to switch to auto-accept mode??

===

check vtcode can't run any commands?
/Users/vinhnguyenxuan/.vtcode/sessions/atif-trajectory-session-vtcode-20260607T032926Z_556918-32490-20260607T032927Z.json /Users/vinhnguyenxuan/.vtcode/sessions/harness-session-vtcode-20260607T032926Z_556918-32490-20260607T032927Z.jsonl

===

| TD-015 | Harness | Checkpoint logs show recovery-mode completion and tool-selection failures: final prose/pseudo-tool output instead of applied mutations, structural-search misuse, large `unified_file` edits, brittle patch context, denied exec retries, overwrite-mode misses, legacy tool aliases, and repeated reads. | P1 | open | 2026-06-07 | Ref: `docs/project/TODO.md#vt-code-agent-harness-robustness-plan`. Counting note: checkpoints are cumulative; use `metadata.prompt_message_index` turn deltas plus unique error dedup, not raw totals across files. Root-cause target for later code pass: `src/agent/runloop/unified/turn/turn_processing/result_handler.rs` recovery `TextResponse` should reject embedded pseudo tool-call markup; `src/agent/runloop/unified/turn/turn_loop.rs` fallback should say no additional tool call was applied. Docs-first next step: update tool usage docs with examples for grep-vs-structural search, edit-vs-overwrite, patch context discipline, policy-denied exec recovery, and recovery-mode “not applied” final wording. Verify docs diff: `git diff -- docs/project/TODO.md docs/harness/TECH_DEBT_TRACKER.md`. |
