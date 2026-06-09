idea design background task UI/UX and logic, configurable in /config.

similar to subagents system design and UI design

```
⎿  Added 2 lines, removed 2 lines
       1 -#ruby=3.1.7  # Informational only — real version pin is in .r
         -uby-version (read by rbenv)
       1 +#ruby=3.3.11  # Informational only — real version pin is in .
         +ruby-version (read by rbenv)
       2  source "https://rubygems.org"
       3
       4  gem "fastlane"
       5  gem 'prettier'
       6  gem 'dotenv-rails'
       7  gem "xcov"
       8 -gem 'cocoapods'
       8 +gem 'cocoapods', '~> 1.16'
       9  gem 'danger'
      10  gem 'danger-swiftlint'
      11

  Ran 1 shell command

⏺ Task Output bcmvpsney
  ⎿  (No output)

⏺ Task Output bcmvpsney
  ⎿  a44400000-a44800000 rw-
     a44800000-a44b00000 rw-
     a47000000-a47600000 rw-
     … +6 lines

⏺ Background command "Update CocoaPods to ~&gt; 1.16 in lockfile" completed
(exit code 0)

  Read 1 file, ran 3 shell commands

  Task Output br6jdags3
  Update CocoaPods in lockfile with Ruby 3.3.11 active
     Waiting for task (esc to give additional instructions)

* Doing… (23m 6s · ↓ 16.2k tokens)

──────────────────────────────────────────────────────────────────────────────
  Shell details

  Status:   running
  Runtime:  4m 29s
  Command:  eval "$(rbenv init - zsh)" && ruby --version && bundle update
            cocoapods 2>&1

  Output:
  ╭──────────────────────────────────────────────────────────────────────╮
  │ ruby 3.3.11 (2026-03-26 revision 1f2d15125a) [arm64-darwin25]        │
  │ Fetching gem metadata from https://rubygems.org/.......              │
  │ Resolving dependencies...                                            │
  │ Resolving dependencies...                                            │
  │                                                                      │
  │                                                                      │
  │                                                                      │
  │                                                                      │
  │                                                                      │
  │                                                                      │
  ╰──────────────────────────────────────────────────────────────────────╯
  Showing 4 lines

  ← to go back · Esc/Enter/Space to close · x to stop
```

---

fix the agent stop and can't work /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_379.json

---

implement control+r to search all prompts with actual logic, reference.

/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/search_prompts.png

---

critical: check and fix screenshot path

─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
udpate readme to showcase screesnshot
Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/resources/screenshots/vtcode-01237.png
////////////////////////////////////////////////////// Error //////////////////////////////////////////////////////
LLM request failed: No endpoints found that support image input
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////
────────────────────────────────────────────────────── Info ───────────────────────────────────────────────────────
Hint: Review error details for specific issues; Check tool documentation for known limitations
───────────────────────────────────────────────────────────────────────────────────────────────────────────────────

==> no need to read the screenshot -> if the prompt is asking to update with image path. also refine and make sure image mention file is robust.

logs:

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

enhanced vtcode's support for ast-grep. also update this note in agents.md and in relevant subagents.

https://ast-grep.github.io/catalog/rust/

---

https://ast-grep.github.io/guide/tools/json.html

==

should we merge vtcode-design, vtcode-themes, vtcode-tui => vtcode-ui? the centralize crate for all UI components and design system. this way we can have a single source of truth for all UI related code and design decisions, and it will be easier to maintain and evolve the UI as a whole. also, it will reduce the number of crates we have to manage and publish, and it will make it easier for contributors to find and work on UI related code.

===

fix auto mode denied policy for running bash command. maybe suggest user to switch to auto-accept mode??

===

check vtcode can't run any commands?
/Users/vinhnguyenxuan/.vtcode/sessions/atif-trajectory-session-vtcode-20260607T032926Z_556918-32490-20260607T032927Z.json /Users/vinhnguyenxuan/.vtcode/sessions/harness-session-vtcode-20260607T032926Z_556918-32490-20260607T032927Z.jsonl

---

## VT Code Agent Harness Robustness Plan {#vt-code-agent-harness-robustness-plan}

Tracked as TD-015 in `docs/harness/TECH_DEBT_TRACKER.md`. Source logs reviewed: `.vtcode/checkpoints/turn_326.json` through `turn_350.json`.

**Counting method**: Checkpoint files are cumulative conversation snapshots. Do NOT sum every error found in every file; that double-counts earlier failures repeated in later checkpoints. Use turn-delta view (count only tool errors after each checkpoint's `metadata.prompt_message_index`) plus unique dedup by `tool_call_id` + error category.

### Corrected Ranked Findings

| Rank | Level | Category | Corrected evidence | Impact | Proposed fix |
|------|-------|----------|--------------------|--------|--------------|
| 1 | P1 | Recovery-mode prose instead of executable action | Turns 347, 349, 350 end with README content or pseudo tool-call markup in assistant text instead of an executed patch/write. | Highest user-visible failure: agent appears to continue work but stops at instructions for the user to apply manually. | Harness completion invariant: if mutating task is requested and tools are unavailable/denied, final output must explicitly say "not applied" and list the blocker. |
| 2 | P1 | Structural-search misuse | 2 new failures in turn 343; 4 unique across snapshots. Examples: `AgentLoop \| agent_loop \| agent loop`, `fn run \| fn execute \| fn tick`, and selector-bearing searches with non-parseable pattern. | Wastes early turn budget and creates noisy recovery loops. | Use grep/text search for prose, regex, symbol names, and alternatives; use structural mode only when the pattern is parseable code. |
| 3 | P1 | Edit primitive mismatch | 1 new failure in turn 343; `unified_file` edit attempted a large README block replacement beyond the small literal-edit limit. | Blocks simple doc changes and can push the agent into fallback loops. | `unified_file` edit is for small literal replacements; larger edits should use patch-style edits with exact context or intentional overwrite. |
| 4 | P1 | Brittle patch context | 1 new failure in turn 344. Patch used synthetic anchor `@@ Real-world workflows @@` rather than exact file context. | Prevents forward progress after partial edits and increases repeated reads. | Read a narrow fresh range, copy exact surrounding context, and re-read when context drifts. |
| 5 | P2 | Policy-denied shell fallback | 2 new failures in turns 348 and 349. `unified_exec` denied for shell-style README inspection. | Wastes turns after policy already gave a hard answer; contributes to recovery-mode prose. | After policy denial, switch to read/search tools or ask the user; do not retry shell inspection. |
| 6 | P2 | Create-vs-overwrite mismatch | 2 new failures in turns 326 and 334. Existing `README.md` and `docs/diagrams/agent-loop-sequence.md` writes omitted intentional overwrite mode. | Recovery was possible but cost extra turns. | Existing-file writes require `mode=overwrite`; otherwise use edit/patch primitives. |
| 7 | P2 | Malformed or legacy tool calls | 3 new failures: 1 malformed XML-style `unified_file` call in turn 333 and 2 legacy `exec` calls in turn 334. | Tool preflight rejects the call before any useful work happens. | Keep tool names explicit in docs and prompts; add recovery hint for unknown legacy aliases like `exec`/`bash`. |
| 8 | P3 | Repeated read loop | 1 new guard trip in turn 349. Repeated full README reads blocked with "Reuse the collected output..." guidance. | Existing guard works, but the agent should switch tactics earlier. | Keep the guard. Add docs guidance to narrow ranges, summarize collected evidence, or re-plan instead of re-reading the same file. |

### Turn-Delta Summary

| Turn | User prompt | New failure categories after `prompt_message_index` | Outcome note |
|------|-------------|------------------------------------------------------|--------------|
| 326 | `revamp the readme` | create-vs-overwrite: 1 | Recovered with overwrite and completed. |
| 327 | slim README agent guide | none new | Completed. |
| 333 | diagram render error | malformed tool call: 1 | Created/fixed diagram but emitted malformed tool XML once. |
| 334 | diagram render error | legacy tool alias: 2; create-vs-overwrite: 1 | Recovered with correct tool/overwrite and completed. |
| 343 | README real use case | structural-search misuse: 2; edit primitive mismatch: 1 | Completed after fallback. |
| 344 | TUI use cases | brittle patch context: 1 | Completed after re-reading exact context. |
| 345 | autonomous capability | none new | Completed. |
| 346 | sponsor section | none new | Completed. |
| 347 | structure/content cleanup | no tool error, but recovery-mode prose/pseudo-apply output | User-visible incomplete application risk. |
| 348 | image border radius | policy-denied exec: 1 | Completed despite denied shell grep. |
| 349 | continue | policy-denied exec: 1; repeated-read guard: 1; recovery-mode pseudo tool-call text | Did not actually apply the full README rewrite. |
| 350 | do it | no new tool error, but final says tools are disabled and provides content to apply | Confirms recovery-mode completion failure. |

### Root Cause: Recovery-Mode Prose Completion

The interactive turn recovery path where tools become unavailable or intentionally disabled produced text containing pseudo tool-call markup (`<tool_call>`, `</tool_call>`, `<function=apply_patch>`, etc.). Because the pseudo call was embedded inside prose instead of emitted as a structured provider tool call, it could be treated as a final assistant answer.

Expected harness behavior:
- Do not execute tools during a tool-free recovery pass.
- Do not display pseudo tool-call markup as if it was applied.
- End with an explicit blocker: "not applied; tools were disabled/denied; latest usable evidence is above; next action is ...".

Surgical implementation points:
- `src/agent/runloop/unified/turn/turn_processing/result_handler.rs`: extend recovery `TurnProcessingResult::TextResponse` guard to detect embedded pseudo tool-call intent across all formats (not only DSML). Markers: `<tool_call`, `</tool_call>`, `<function=`, `<parameter=`, `<invoke name=`, MiniMax tool-call tags.
- `src/agent/runloop/unified/turn/turn_loop.rs`: make the deterministic recovery fallback explicitly say no additional tool call was applied.

### Status

- **Code pass**: Done. Recovery `TextResponse` strips detected non-DSML pseudo tool-call regions when usable prose remains; markup-only output still falls back; recovery fallback says "No additional tool call was applied"; policy-denied tool errors include `/mode auto` guidance.
- **Docs pass**: Done. `docs/tools/TOOL_SPECS.md` and `docs/development/EXECUTION_POLICY.md` updated with examples for grep-vs-structural search, edit-vs-overwrite, patch context discipline, policy-denied exec recovery, and recovery-mode "not applied" wording.
- **Future**: Evaluator checkpoint metrics for the eight failure categories above.
