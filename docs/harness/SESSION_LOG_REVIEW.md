# Session Log Review

## 2026-10-11 | Semantic operation identity bounds repeated preflight rejections across turns

### Baseline

Session `session-vtcode-20261011T073126Z_754860-74519` ("use plan mode, refine and improve vtcode @README.md", 19 turns,
~2.6M input tokens) ran 17 autonomous continuation turns with 43 failed tool calls. 35 of 39 `task_tracker` calls were
`action:"list"` carrying forbidden `index`/`index_path` (plus varying `status`/`max_output_tokens`); every turn ended
in turn-loop auto-continuation, and the harness never stopped.

| Observation                                                                                                                                                             | Disposition                                                                                                                                                                                                                               |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `PreflightRecovery::reject` compared full JSON equality, so each cosmetic variant (`status`, `max_output_tokens`) counted as a new rejection and the cap-6 distinct budget never exhausted. | `reject` now reuses `operation_matches` (shared with `succeeded`) so repeat detection is semantic: tool + action + selectors. `list` variants collapse to one identity; distinct `update` targets stay distinct. Malformed-JSON rejects repeat on tool+category. |
| The live-session fixture (`tracker_preflight_session.json`) pinned the old exact-dedup outcome (8 calls, 3 exact duplicates).                                             | Fixture now stops after 7 calls / 2 exact duplicates; unit regression `preflight_cosmetic_field_variations_repeat_the_same_operation` covers the variant-repeat shape directly.                                                             |
| `create`-variant items-text differences don't change the operation (title identity), so two distinct drafts collapse correctly.                                            | No change to `tracker_operation_matches`; title-based identity already covers it.                                                                                                                                                          |

### Verification

<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode -E 'test(preflight)'` — 38 passed (incl. fixture + new unit regression)
- `cargo nextest run -p vtcode --profile quick -E 'test(tracker) or test(continuation) or test(auto_continue) or test(cross_turn) or test(turn_loop)'` — 365 passed
- `cargo clippy --locked -p vtcode --all-targets -- -D warnings` — clean; `cargo fmt --all -- --check` — clean
- Pre-existing unrelated failure (also on clean base): `picker_separates_openrouter_reasoning_from_effort_support`

## 2026-10-11 | Per-item bracket `verify:` list terminally rejected a valid README plan

### Baseline

Session `session-vtcode-20261011T125442Z_154501-05568` ("use plan mode, refine and improve vtcode @README.md") ended
its only turn with `PLANNING_PLAN_REJECTION_NOTICE` and
`step 3: verification item 2 must be a concrete command or check`, exhausting the bounded repair budget instead of
reaching approval. The model emitted each verification command in its own bracket group:
`verify: [npx markdownlint-cli2 README.md], [git diff -- README.md]`.

| Observation                                                                                                                                        | Disposition                                                                                                                                                                                                                                                                                                                                                                                              |
| -------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `parse_bracket_list` stripped only the outermost `[`/`]` *before* `split_bracket_items`, so the per-item group form fragmented into `npx markdownlint-cli2 README.md]` and `[git diff -- README.md`; item 2 then failed as non-concrete. | `parse_bracket_list` now detects a single whole-string bracket group (`whole_bracket_group`, quote- and bracket-aware) and otherwise splits on top-level commas (bracket-depth aware, `split_top_level_bracket_items`) before unwrapping one balanced pair per entry (`unwrap_bracket_group`). `split_bracket_items` still serves the whole-group body and the three tracker callers. |
| The prior 2026-09-30 issue was the whole-group form `[a, b]` with `npx` missing from `COMMAND_NAMES`; this is a distinct bracket-shape bug, not an allowlist gap. | No allowlist change. Regressions: `per_item_brackets_in_verify_list_validate_as_one_plan` (exact live plan) and `parse_bracket_list_supports_whole_group_and_per_item_forms` (whole-group, per-item, mixed, bare, nested-comma, `[]`).                                                                          |
| The earlier-session rate-limit `trajectory-20261011T125442Z.jsonl` (`ts` 09:47:56Z) is a rotated file whose name reflects rotation time, not this session; it is not the reported failure. | Confirmed non-cause; the reported session's only turn failed on plan validation.                                                                                                                                                                                                                       |

### Verification

<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode-core -E 'test(planning) or test(tracker) or test(bracket) or test(artifact) or test(verify)'` — 274 passed
- `cargo nextest run -p vtcode -E 'test(plan) or test(repair) or test(reject) or test(tracker) or test(planning)'` — 556 passed
- `cargo clippy --locked -p vtcode-core --all-targets -- -D warnings` — clean
- `cargo fmt --all -- --check` — clean
- `./scripts/check-dev.sh` — PASS

## 2026-09-19 | Post-merge review: inspection verify allowlist tightening

### Baseline

Merged planning-verify-commands accepted inspection commands as concrete `verify:` items. Follow-up full-diff review
confirmed an unintended validation regression: `COMMAND_NAMES` now includes English-word binaries (`file`, `sort`,
`find`, `ls`, `make`, …), and the command-head rule only required a second token, so prose verifies such as
`file changes` validated as concrete. Example lists were also duplicated across prompt surfaces without a sweep test.

| Observation                                                                                                                              | Disposition                                                                                                                                                                                                   |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `validate_concrete_verification("file changes")` / `sort order` / `find files` / `make sense` returned Ok after the allowlist expansion. | Ambiguous English command heads now require a later flag (`-n`) or path-like token. Unambiguous heads (`cargo`, `rg`, `sed`, `grep`) keep the multi-token rule so `cargo test` stays valid.                   |
| Valid/invalid verify examples drifted across compiled prompt constants.                                                                  | Shared `PLANNING_VERIFY_VALID_EXAMPLES` / `PLANNING_VERIFY_INVALID_EXAMPLES` in `planning_workflow::artifacts`; `repair_feedback()` embeds them; presence sweep test covers shipped recovery/repair surfaces. |
| `COMMAND_NAMES` is plan-validation only (not shell safety).                                                                              | No security-path change; document remains scoped to plan verification.                                                                                                                                        |

### Verification

<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode-core -E 'test(planning) or test(inspection) or test(agentic) or test(validate_plan) or test(plan_quality) or test(english_phrase)'`
- `cargo nextest run -p vtcode -E 'test(shipped_planning_prompt) or test(repair_directive) or test(planning)'`
- `cargo check --locked -p vtcode-core -p vtcode`
- `cargo fmt --all -- --check`

## 2026-09-19 | Planning recovery rejects inspection-command verifies

### Baseline

Live blocker `session-vtcode-20260918T101837Z_175560-33614` ended planning via recovery fallback without an
approval-ready plan (324k prompt tokens). Checkpoints `turn_1234`–`turn_1243` show the same class of failure on
README/docs plans:
`invalid plan artifact: invalid implementation steps: … verification item 1 must be a concrete command or check`.
Rejected `verify:` values were ordinary inspection commands (`sed -n`, `grep -n`) that planning research itself uses.
The validator allowlist `COMMAND_NAMES` in
`crates/codegen/vtcode-core/src/tools/handlers/planning_workflow/artifacts.rs` included `cargo`/`rg`/`python3` but
omitted `sed`, `grep`, `head`, `tail`, `wc`, `cat`, `find`, and related inspection tools, so recovery synthesis produced
concrete drafts that failed validation and burned the bounded repair budget.

| Observation                                                                                                                                                        | Disposition                                                                                                                                                                                                                                                                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `validate_concrete_verification("sed -n '81,88p' README.md")` returned `InvalidItem { ordinal: 1 }`.                                                               | Expanded `COMMAND_NAMES` with inspection/verification binaries: `sed`, `grep`, `egrep`, `fgrep`, `head`, `tail`, `wc`, `cat`, `find`, `ls`, `awk`, `sort`, `uniq`, `cut`, `tr`, `diff`, `jq`, `stat`, `file`. `git diff --check` and vague prose stay rejected.                                                                               |
| Recovery/repair prompt surfaces only listed `cargo`/`rg` valid examples, steering synthesis away from inspection commands even when they were legal for docs work. | Synced valid examples in `PLANNING_SYNTHESIS_FORMAT_HINT`, `repair_feedback()`, `PLANNING_WORKFLOW_PLAN_QUALITY_LINE`, `PLANNING_COMPLETED_FALLBACK_RESPONSE`, `POST_TOOL_RECOVERY_REASON_PLAN_MODE`, `RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE`, and the denied-interview / recovery-handoff directives to include `sed -n` / `grep -n`. |
| Quoted-comma `sed` ranges already parsed as one bracket item (2026-09-14 fix) but still failed as non-concrete commands.                                           | Regression tests now assert the live checkpoint verifies validate as ready plans; quoted-comma `sed` remains one concrete item.                                                                                                                                                                                                               |

### Verification

<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode-core -E 'test(planning_workflow) or test(validate_plan) or test(inspection) or test(quoted_comma) or test(agentic_testing) or test(plan_quality)'`
- `cargo nextest run -p vtcode -E 'test(repair) or test(planning)'`
- `cargo check --locked -p vtcode-core -p vtcode`
- `cargo fmt --all -- --check`

## 2026-09-14 | Session session-vtcode-20260914T031505Z_075199-09813 planning validation audit

### Baseline

Reviewed `.vtcode/sessions/session-vtcode-20260914T031505Z_075199-09813/events.jsonl` (85 events, 2 turns),
`.vtcode/logs/trajectory-20260914T031505Z.jsonl`, `.vtcode/checkpoints/turn_1171.json` / `turn_1172.json`,
`.vtcode/tool-policy.json`, and `.vtcode/history/session-vtcode-20260914T031505Z_.memory.json`. Turn 1171 ended with
`invalid plan artifact: invalid implementation steps: step 4: verification item 1 must be a concrete command or check`;
turn 1172 hit the planning-gate `apply_patch` denial, then terminal rejection with the same reason.

| Observation                                                                                                                                                                                                                                   | Disposition                                                                                                                                                                                                                                                                          |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Step 4 verify `[sed -n '/^## Documentation/,/^## Development/p' README.md \| wc -l ...]` was split into two bracket items at the comma inside the quoted `sed` address range, so item 1 (`sed -n '/^## Documentation/`) failed as a fragment. | `parse_bracket_list` now splits only on commas outside single/double quotes (`split_bracket_items`); the quoted `sed` range stays one item. Regression tests: `bracket_split_keeps_quoted_commas_inside_one_item`, `validate_plan_content_keeps_quoted_comma_verify_as_single_item`. |
| `repair_feedback` already required each comma-separated verify entry to be concrete but never stated the quoting rule.                                                                                                                        | Repair text now notes commas inside single/double quotes stay inside one item (validator-owned string, no raw echo).                                                                                                                                                                 |
| Dispatch/admission counters in turns 1171–1172 were sane; tool-policy gating was correct (mutating tools denied while unapproved).                                                                                                            | No dispatch change — validation-only fix.                                                                                                                                                                                                                                            |

### Verification

<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode-core -E 'test(planning_workflow) or test(validate_plan) or test(bracket) or test(quoted_comma) or test(agentic_testing)'`
- `cargo check --locked -p vtcode-core`

## 2026-08-16 | Checkpoints 912–917 diagnostics audit

### Baseline

Reviewing `.vtcode/checkpoints/turn_912.json` through `turn_917.json` showed every turn that executed tools reported
`requested_tool_calls: 0` alongside a non-zero `admitted_tool_calls` (26 in turn 912, 52 in turn 913, 2 in turn 917),
making the requested/admitted ratio unusable for admission-rate diagnostics.

| Observation                                                                                  | Disposition                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `requested_tool_calls` stayed 0 on interactive turns while `admitted_tool_calls` grew.       | `record_requested_tool_calls` was only called on the full-auto batch path (`handle_tool_call_batch_prepared`); `record_admitted_tool_call` increments in the shared permission gate reached from every path. Recording moved to the single dispatch entry point (`handle_tool_calls`) so batch and single-call paths count identically; the duplicate call in the batch handler was removed. Regression test: `single_tool_call_dispatch_records_requested_tool_calls`. |
| Turn 917 reported `cached_input_tokens: 0, cache_creation_tokens: 0` on ~36k input.          | Not a harness defect: `accumulate_turn_usage` sums usage across all calls in the turn and recovery messages are appended without mutating the history prefix; the zeros are provider-reported (endpoint does not return cache fields).                                                                                                                                                                                                                                  |
| Turn 916's session-memory envelope carried a task tracker from an unrelated prior objective. | By design: the task tracker is durable workspace state (`.vtcode/tasks/current_task.md`) shared across sessions for long-horizon resume.                                                                                                                                                                                                                                                                                                                                |
| Turn 916 history contains consecutive user messages ("hello", "Hello").                      | Normal resubmission after an aborted turn; the first submission stays in history.                                                                                                                                                                                                                                                                                                                                                                                       |

### Verification

- `cargo check --locked`: clean. `cargo nextest run --profile quick -p vtcode`: 2,356 tests pass, including the new
  dispatch counter regression test.

## 2026-08-12 | Planning wire-catalog collapse (turns 912–913)

### Baseline

The planning turns 912 and 913 ("make a simple plan to improve vtcode launch time") each burned ~157k input tokens
across 54 `code_search` calls, persisted no plan, and ended with the planning reminder bullet as the only visible
assistant text.

| Root cause                                                                                                                                                                                                                       | Fix                                                                                                                                                                                                                                                                                                                                                                                                                         |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The plan agent's permission rules (`default: deny, allow: [read]`) denied the `exec_command` (Bash) and `request_user_input` (Other) advertisement probes, so wire shaping collapsed the planning catalog to bare `code_search`. | Split the permission ladder: `readonly_agent_permissions` (`read`), `readonly_interview_agent_permissions` (+`request_user_input`), `plan_agent_permissions` (+`bash`). Read-only enforcement stays with the planning dispatch gate, which hard-blocks mutating commands before execution; `resolve_approved_plan_execution_agent` now excludes `plan` by name since its permission heuristic no longer reads as read-only. |
| The planning profile admitted only `exec_command`/`code_search`/`request_user_input`, and the builtin read-only tool lists omitted `grep_file`/`read_file`/`list_files`, so agent tool-policy filtering re-hid them.             | Planning profile and builtin read-only tool lists now include the direct read tools (Interactive surface still hides `read_file`/`list_files`; planners read files via gated `exec_command`). Explorer gains real read tools too — it had the same latent single-tool collapse.                                                                                                                                             |
| The initial system prompt described valid plan steps but never showed the canonical grammar; the example appeared only in the post-rejection repair directive.                                                                   | `PLANNING_WORKFLOW_PLAN_QUALITY_LINE` embeds `CANONICAL_STEP_FORMAT` verbatim (sync enforced by test).                                                                                                                                                                                                                                                                                                                      |
| A rejected `<proposed_plan>` vanished from history — the repair retry could not see what it was fixing, and terminal rejections left checkpoints/events with only the planning reminder.                                         | `reject_plan_artifact` re-attaches the bounded (8 KiB) rejected draft to the stored assistant message on both repair and terminal paths.                                                                                                                                                                                                                                                                                    |
| Session archives recorded every `code_search` result as "Structured result with fields: query, filters, results, returned".                                                                                                      | `tool_output_payload_from_value` summarizes `results` arrays (count, query, up to 3 `path:line` samples).                                                                                                                                                                                                                                                                                                                   |

### Verification

- `vtcode-config`: 389 tests pass, including updated builtin-agent contract tests.
- `vtcode-core`: new `builtin_plan_agent_keeps_planning_catalog_wire_visible` (permission-layer wire shaping against the
  real plan spec), planning-catalog profile tests (Interactive + AgentRunner surfaces),
  `planning_workflow_keeps_exec_command_read_only`, prompt canonical-format guard, and `results`-summary tests.
- Binary: rejected-draft reattachment unit tests and `plan_agent_is_never_an_execution_handoff_target`.
- `./scripts/check-dev.sh --changed`: clean (5,634 tests).

## 2026-08-12 | Checkpoints 862–911 stabilization

### Baseline

The 50 active turns in checkpoints 862–911 produced 507 tool calls, 57 structured failures, 124 spooled or
reference-only results, and about 1.40 MB of model-visible tool output. `code_search` accounted for 245 calls: 93 empty
results (38%) and 150 truncated results (61%). Seven historical turns ended without a final answer.

The most recent examples showed that the defects were still live:

| Turn | Observation                                                                                                                                                                  |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 895  | Repeated low-signal searches continued without an early synthesis opportunity.                                                                                               |
| 910  | The turn consumed 51,523 input tokens.                                                                                                                                       |
| 911  | A project-orientation request used 9 commands, hit 2 avoidable permission-validation failures, exposed about 139 KB of raw spooled output, and consumed 43,282 input tokens. |

### Stabilized behavior

- The planning workflow retains its 120-call hard ceiling. A separate adaptive guard schedules one tool-free synthesis
  pass after 6 consecutive or 10 total low-signal navigation results. A productive read resets only the consecutive
  count; verification, mutation, recovery, and a new turn reset both counts.
- Shell progress accounting distinguishes inspection (`rg`, `find`, `cat`, simple `sed -n`, and similar reads),
  verification (compile, build, test, and clippy families), and mutation. Inspection contributes to navigation
  accounting instead of masquerading as implementation progress.
- Successful spooled inspection output exposes a deterministic UTF-8-safe head/tail preview within 6 KB and 80 lines.
  Build, test, and runtime output uses a tail-focused preview within the same 6 KB/80-line envelope. A lower caller
  output budget wins. Structured `failure_diagnostics` stays reference-only, and every preview retains its `spool_path`
  for a targeted follow-up.
- `exec_command` normalizes omitted or `use_default` sandbox mode with non-empty `additional_permissions` to
  `with_additional_permissions`. Requested paths still pass the normal traversal, symlink-containment, sensitive-path,
  and allowed-root validation. Explicit escalation or bypass conflicts remain fail-closed.
- Newly written V2 snapshots can link session and runtime turn identifiers and include per-turn diagnostics for
  authoritative token usage, elapsed time, tool admission/outcomes, spooling, model-visible bytes, low-signal calls, and
  recovery activation. V0/V1 snapshots migrate with those additive fields absent.

These changes preserve public tool names, sandbox permission enum values, duplicate-signature guards, and hard tool-call
ceilings. `vtcode-exec-events::ThreadEvent` remains the authoritative runtime accounting contract.

## 2026-08-11 | Harness token-waste and startup-noise audit

### Scope

Reviewed legacy session logs under the historical `VTCODE_HOME/sessions/` path:

- `debug-session-vtcode-20260811t063743z_863637-66412.log` (startup debug log)
- `harness-session-vtcode-20260808T223648Z_961150-70620-*.jsonl` (2.6 MB, 3 turns, 3253 events)
- `harness-session-vtcode-20260731T234131Z_673442-70655-*.jsonl` (1.1 MB, comparison baseline)

Goal: identify bottlenecks, duplicate work, wasted tokens, and log noise that degrade long-running harness stability.

### Findings and disposition

| #   | Severity   | Finding                                                                                                                                                  | Root cause                                                                                                                                                  | Disposition                                                                                                                                                                                                                                          |
| --- | ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | **High**   | 3,069 `item.updated` events for 49 tool calls (avg 62.6, worst 564) in a single 3-turn session; ~1.34 MB of repeated argument payloads in the JSONL log. | Every streaming argument delta (one per token) emitted a full-arguments `item.updated` event with no rate limit or cap.                                     | **Fixed**: throttled in `lifecycle.rs` — first delta emits eagerly, subsequent only after 512 new bytes, capped at 8 intermediate updates. `complete_tool_call` always emits the full final arguments. Estimated 7× reduction for the worst session. |
| 2   | **Medium** | 162 `WARN`-level "Unknown SKILL.md frontmatter key" messages at startup (~21 KB log noise), repeating the full supported-key list each time.             | `validate_frontmatter_keys` called `trim()` before checking column position, so nested keys under supported parents (`metadata:`, etc.) appeared top-level. | **Fixed** in `manifest.rs`: only column-zero keys are checked; unknown keys are deduplicated and emitted as one consolidated warning per manifest.                                                                                                   |
| 3   | **Medium** | 6 `WARN`-level "Failed to connect to … server" messages at startup for Ollama, llama.cpp, and LM Studio when none are running.                           | Inventory fetch functions (`fetch_ollama_models`, `fetch_llamacpp_models`, `fetch_lmstudio_models`) logged at `WARN` for expected connection-refused.       | **Fixed**: downgraded to `DEBUG` in the three inventory-fetch paths. Active-use path (`OllamaClient::probe_server`) retains `WARN` because the user explicitly chose the provider.                                                                   |
| 4   | **Low**    | Repeated `TRACE "Registered custom provider"` (4× in one session) from `register_custom_providers` called on every turn via `refresh_vt_config`.         | The sync/replace function removes and re-registers all custom providers unconditionally, even when the config hasn't changed.                               | **Fixed**: `refresh_vt_config` now compares old vs new `custom_providers` (enabled by adding `PartialEq, Eq` to `CustomProviderConfig`) and skips the remove+register cycle when unchanged.                                                          |

### Non-issues (false positives filtered)

- **Local-provider probes run eagerly at startup**: verified false. All probe/fetch call sites are on-demand (model
  picker, `/local` commands, generation-time readiness). The startup warnings came from `DynamicModelRegistry::load`
  called when the model picker initializes, not from an eager startup probe loop.
- **`atlascloud` duplicate registry state**: verified false. The factory's `register_custom_providers` removes all
  non-builtin providers before re-registering; there is no duplicate state, only redundant work. Fix #4 above eliminates
  the redundant work.
- **Persistence-layer filtering needed separately**: decided against. Source-level throttling (fix #1) is simpler,
  preserves live progress UI, and keeps replayed and live event streams consistent. A second persistence-level filter
  would risk divergence.

### Measured impact (fix #1)

| Metric                                | Before   | After (estimated)                      |
| ------------------------------------- | -------- | -------------------------------------- |
| `item.updated` events (worst session) | 3,069    | ~441 max (49 calls × 9)                |
| Avg updates per tool call             | 62.6     | ≤ 8 + 1 completion                     |
| Worst single-call updates             | 564      | ≤ 8 + 1 completion                     |
| Argument bytes in JSONL               | ~1.34 MB | bounded to ~49 × 9 × 2 KB ≈ 882 KB max |

### Verification commands

```text
cargo fmt --all -- --check
cargo nextest run -p vtcode-skills -E 'test(manifest)'
cargo nextest run -p vtcode-core -E 'test(lifecycle)'
cargo nextest run -p vtcode-config -p vtcode-llm --profile quick
cargo clippy --locked -p vtcode-skills -p vtcode-core --all-targets -- -D warnings
cargo clippy --locked -p vtcode-config -p vtcode-llm -p vtcode --all-targets -- -D warnings
cargo check --locked -p vtcode
```

All passed.

### Files changed

| File                                                            | Change                                                                                                                                        |
| --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/codegen/vtcode-core/src/core/agent/events/lifecycle.rs` | Throttled tool-argument `item.updated` emissions; added `last_emitted_args_len`/`update_events` to `ToolCallStreamState`; 2 regression tests. |
| `crates/codegen/vtcode-skills/src/manifest.rs`                  | Column-zero-only frontmatter key validation; consolidated warning; 5 regression tests.                                                        |
| `crates/codegen/vtcode-llm/src/providers/ollama.rs`             | `fetch_ollama_models` connection-refused log: `WARN`→`DEBUG`.                                                                                 |
| `crates/codegen/vtcode-llm/src/providers/llamacpp/probe.rs`     | `fetch_llamacpp_models` connection-refused log: `WARN`→`DEBUG`.                                                                               |
| `crates/codegen/vtcode-llm/src/providers/lmstudio.rs`           | `fetch_lmstudio_models` connection-refused log: `WARN`→`DEBUG`.                                                                               |
| `crates/codegen/vtcode-config/src/core/custom_provider.rs`      | Added `PartialEq, Eq` derives to `CustomProviderConfig`.                                                                                      |
| `src/agent/runloop/unified/turn/workspace.rs`                   | Skip `register_custom_providers` when `custom_providers` unchanged.                                                                           |

---

## 2026-08-11 | Harness core logic audit (DRY/KISS, brittleness, long-running stability)

### Scope

Deep audit of the actual harness core logic in `crates/codegen/vtcode-core/src/core/agent/` (runtime, session, error
recovery, progress monitor, runner). Goal: find systemic patterns — duplicated logic, dead code, wasted work, tight
coupling, long-running stability risks — and mitigate with KISS/DRY fixes. Focus on production/main code.

### Findings and disposition

| #   | Severity   | Finding                                                                                                                                                                                                                                                                                                                                                                           | Root cause                                                                                                                                                                                      | Disposition                                                                                                                                                                                                                                                                                                                                                                               |
| --- | ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 5   | **High**   | Tool-argument throttle (fix #1) broke small tool calls: the first delta emits eagerly with incomplete JSON, and the completing delta (below 512 B) is throttled out, so the full streamed arguments never appear in an `item.updated` before tool execution.                                                                                                                      | The throttle emitted the first delta unconditionally and gated subsequent deltas on byte growth only; there was no final flush of the authoritative accumulated arguments when streaming ended. | **Fixed**: added `SharedLifecycleEmitter::flush_open_tool_call_arguments` which emits a final `item.updated` with the full accumulated arguments for each open tool call (skipping redundant flushes when the last intermediate update already captured the full args). `StreamingLifecycleBridge::complete_open_items` now calls it before closing text items. 2 regression tests added. |
| 6   | **Medium** | `session/loop_detection.rs` (`LoopDetectionState`, ~210 lines) was dead code — never instantiated outside its own tests — and had divergent semantics from the live `AgentSessionState` loop logic (e.g. it reset `tool_loop_limit_hit` on every LLM response and used a different stagnation algorithm). A maintainer wiring it in would have silently changed harness behavior. | A prior extraction attempt was never connected to the live state; the live `AgentSessionState` kept its own inline fields and methods.                                                          | **Fixed**: deleted `session/loop_detection.rs` and its `pub mod loop_detection;` export. The live `AgentSessionState` loop-detection logic is the single source of truth.                                                                                                                                                                                                                 |
| 7   | **Medium** | Duplicated streaming-update throttle state and policy across `StreamingLifecycleBridge` and `AgentRuntime` (4 fields + 2 methods each, sharing the same constants but with subtly different eager conditions).                                                                                                                                                                    | Two streaming paths evolved the same throttle independently.                                                                                                                                    | **Fixed**: extracted a single `UpdateThrottle` struct (`should_emit`/`record`/`advance_to`/`is_first`/`reset`) that both structs compose. All four original behaviors (output first-eager, reasoning first-eager, reasoning stage-change-eager, cap) are preserved exactly. 5 unit tests added for the throttle in isolation.                                                             |
| 8   | **Low**    | `ErrorRecoveryState::detect_error_patterns` was computed eagerly inside `get_diagnostics` on every recovery diagnostics call, but `RecoveryDiagnostics.error_patterns` is never read by any production path (only by one unit test). Dead computation on the circuit-breaker recovery path.                                                                                       | `get_diagnostics` unconditionally built a per-tool error histogram.                                                                                                                             | **Fixed**: `get_diagnostics` now returns an empty `error_patterns`; `detect_error_patterns` is public so callers can request pattern analysis explicitly. Updated the unit test to call it directly.                                                                                                                                                                                      |
| 9   | **Low**    | `ErrorRecoveryState::circuit_events` used `Vec::remove(0)` (O(n) front removal) while the sibling `recent_errors` field used `VecDeque::pop_back` (O(1)). Inconsistent bounded-ring-buffer implementation.                                                                                                                                                                        | Two bounded queues in the same struct used different collection types.                                                                                                                          | **Fixed**: `circuit_events` is now `VecDeque` with `push_back`/`pop_front`, matching `recent_errors`.                                                                                                                                                                                                                                                                                     |

### Non-issues (false positives filtered)

- **Escalation/stall counter reset semantics**: investigated `consecutive_escalations` increments/resets across
  `runner/execute.rs` and `session/mod.rs`. The counter resets only when tool calls pass the escalation gate (forward
  progress) or on full history clear — intentional and correct. Not a long-running stability bug.
- **Progress monitor unbounded queues**: `SessionProgressSink` uses a `sync_channel(1)` with coalescing
  (`pending.ledger` is overwritten, `checkpoint` is OR'd). No unbounded growth; the writer thread exits cleanly when the
  sender is dropped. Correct best-effort design.
- **`execute_task` monolith**: `runner/execute.rs::execute_task` is large but already decomposed into
  `continuation_controller`, `resolve_completion_assessment`, and batch helpers. The stop/continue/completion decisions
  are interleaved with I/O and side effects; a pure extraction would be risky without deeper analysis. Left untouched
  per "do not split by arbitrary line count."

### Verification commands

```text
cargo fmt --all -- --check
cargo nextest run -p vtcode-core -E 'test(runtime) | test(update_throttle) | test(lifecycle) | test(error_recovery) | test(flush_open_tool_call) | test(append_tool_call_delta)'
cargo nextest run -p vtcode --no-fail-fast -E 'test(harness_streaming_bridge)'
cargo nextest run --profile quick -p vtcode-core -p vtcode
cargo clippy --locked -p vtcode-core -p vtcode --all-targets -- -D warnings
```

All passed: 5573 tests across `vtcode-core` + `vtcode`, clippy clean with `-D warnings`, formatting clean.

### Files changed

| File                                                                  | Change                                                                                                                                                       |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `crates/codegen/vtcode-core/src/core/agent/events/lifecycle.rs`       | Added `flush_open_tool_call_arguments` to emit full accumulated args for open tool calls; 2 regression tests.                                                |
| `crates/codegen/vtcode-core/src/core/agent/runtime/mod.rs`            | Extracted `UpdateThrottle` shared by `StreamingLifecycleBridge` and `AgentRuntime`; `complete_open_items` now flushes tool-call args; 5 throttle unit tests. |
| `crates/codegen/vtcode-core/src/core/agent/session/mod.rs`            | Removed dead `pub mod loop_detection;` export.                                                                                                               |
| `crates/codegen/vtcode-core/src/core/agent/session/loop_detection.rs` | Deleted (dead code with divergent semantics).                                                                                                                |
| `crates/codegen/vtcode-core/src/core/agent/error_recovery.rs`         | `circuit_events` → `VecDeque` with `pop_front`; `detect_error_patterns` now public and lazy (not computed in `get_diagnostics`); updated unit test.          |

---

## 2026-08-11 | Continued harness audit: task_tracker `view` token waste + abort-path verification

### Scope

Continuation of the harness core audit. Reviewed remaining session-log waste patterns across 100 legacy session files
under `VTCODE_HOME/sessions/`, verified the abort/cancel streaming-termination path, and confirmed the `AgentRuntime` vs
`StreamingLifecycleBridge` divergence is intentional.

### Findings and disposition

| #   | Severity        | Finding                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | Root cause                                                                                                                                                                                                                                                                                                                                                                                         | Disposition                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| --- | --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 10  | **Medium-High** | `task_tracker` (and its planning-workflow variant) returns a `view` field — TUI display data (branch symbols, status icons, per-item display lines) that duplicates the structured `checklist` already in the same payload. The full JSON including `view` is serialized into LLM context on every `create`/`update`/`add`/`list` call. Session logs show results growing from ~5 KB to ~12 KB per call as items accumulate `outcome`/`verify` metadata, with `view` accounting for ~3 KB of each — pure redundancy the model never acts on. | The tool handler returns one `Value` used for both TUI rendering and LLM context; no layer stripped TUI-only fields before model serialization. `reduce_tool_result` only truncates `read_file`/`exec` output; `compact_model_tool_payload` strips exec fields but not `view`.                                                                                                                     | **Fixed**: added `strip_tui_display_fields(tool_name, value) -> Cow<Value>` in `result_reducers.rs` (borrows unchanged for non-tracker tools, owns a `view`-stripped copy for tracker tools). Wired into both production paths: `AgentRunner::apply_tool_success` (before `push_tool_result`, event keeps `view`) and the unified runloop's `prepare_tool_response_content` (TUI reads `view` from the original `output` via the pipeline-output path, not from this string). 5 unit tests added. |
| —   | _Non-issue_     | `StreamingLifecycleBridge::abort()` does not call `flush_open_tool_call_arguments` before `complete_open_tool_calls_with_status(Failed)`.                                                                                                                                                                                                                                                                                                                                                                                                    | Investigated: `complete_tool_call` (called by `complete_open_tool_calls_with_status`) emits the `tool_invocation_completed` event **with the full accumulated arguments** inline, so a flush before it would be redundant. The flush in `complete_open_items` (success path) is needed only because tool calls stay open (InProgress) after streaming — the abort path completes them immediately. | **No change** — abort path is correct and consistent.                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| —   | _Non-issue_     | `AgentRuntime::record_model_progress` and `StreamingLifecycleBridge::on_progress` duplicate streaming-progress→lifecycle bridging logic with subtly different reasoning-stage/reasoning-started handling.                                                                                                                                                                                                                                                                                                                                    | The two paths serve different consumers (core runner vs binary harness) and have intentionally diverged: the bridge throttles stage updates via `MAX_REASONING_UPDATE_EVENTS=2` and uses `reasoning_started` gating; the runtime allows unlimited stage updates and uses `is_first` eager. Both now share `UpdateThrottle` for the accounting (fix #7).                                            | **No change** — merging further would require reconciling intentional semantic differences; the `UpdateThrottle` extraction was the right DRY boundary.                                                                                                                                                                                                                                                                                                                                           |
| —   | _Non-issue_     | Duplicate tool calls across sessions (same tool + same args, e.g. `cat src/main.rs` ×2, `cargo build` ×2).                                                                                                                                                                                                                                                                                                                                                                                                                                   | Model-level re-issues across different turns — the model re-decides to run the same command after other work. The harness already has loop-detection, read-after-write guards, and `record_successful_readonly_signature` for turn-local dedup.                                                                                                                                                    | **No change** — expected model behavior, not a harness defect.                                                                                                                                                                                                                                                                                                                                                                                                                                    |

### Verification

```text
cargo check --locked -p vtcode-core -p vtcode
cargo nextest run --profile quick -p vtcode-core -E 'test(strip_view) | test(result_reducers) | test(task_tracker) | test(update_throttle) | test(streaming_lifecycle) | test(flush_open_tool_call) | test(recovery)'
cargo nextest run --profile quick -p vtcode -E 'test(tool_output) | test(tracker) | test(harness_streaming_bridge) | test(response_content) | test(task_tracker)'
cargo nextest run --profile quick -p vtcode-core -p vtcode
cargo clippy --locked -p vtcode-core -p vtcode --all-targets -- -D warnings
cargo fmt --all -- --check
```

All passed: 5,578 tests across `vtcode-core` + `vtcode`, 62 focused core tests, 334 focused binary tests, clippy clean,
formatting clean.

### Files changed

| File                                                            | Change                                                                                                                          |
| --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `crates/codegen/vtcode-core/src/core/agent/result_reducers.rs`  | Added `strip_tui_display_fields` — strips TUI-only `view` from `task_tracker` results before model serialization; 5 unit tests. |
| `crates/codegen/vtcode-core/src/core/agent/harness_kernel.rs`   | Re-export `strip_tui_display_fields` alongside `reduce_tool_result`.                                                            |
| `crates/codegen/vtcode-core/src/core/agent/runner/tool_exec.rs` | `apply_tool_success` strips `view` before `push_tool_result`; the TUI event still uses the full result with `view`.             |

---

## 2026-08-11 | Continued harness audit: fallback DRY, failure-diagnostics tail skip, circuit-breaker dedup, dead tracking code

### Scope

Further continuation of the harness core audit. Traced remaining token-waste patterns from session logs, verified DRY
consistency across both production harness paths, and removed dead parallel abstractions.

### Findings and disposition

| #   | Severity   | Finding                                                                                                                                                                                                                                                                                                                                          | Root cause                                                                                                                                                                                                 | Disposition                                                                                                                                                                                             |
| --- | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 11  | **Medium** | `apply_fallback_success` in `runner/tool_exec.rs` pushed the reduced result to the LLM conversation **without** calling `strip_tui_display_fields`, while `apply_tool_success` did. A fallback from a tool that produces a `view` field would leak TUI display data into model context.                                                          | The fallback success path was added without the strip call that was later added to the main success path.                                                                                                  | **Fixed**: `apply_fallback_success` now calls `strip_tui_display_fields` before `push_tool_result`, matching `apply_tool_success`. The TUI event still uses `optimized_result` with `view`.             |
| 12  | **Medium** | `write_stdin`/`exec_command` results with structured `failure_diagnostics` (cargo test failures) still embed a ~10 KB `tail_preview` of raw test-runner output — mostly thousands of PASS lines. The diagnostics already contain the panic message, source location, rerun hint, and next action, making the tail preview redundant token waste. | `maybe_inline_spooled_with_preview` unconditionally read the spool file and embedded a tail preview whenever `result_ref_only` was set, regardless of whether structured diagnostics were already present. | **Fixed**: `maybe_inline_spooled_with_preview` now skips the tail preview when `failure_diagnostics` is present and non-null. The model can still read the spool file directly if needed. 1 test added. |
| 13  | **Low**    | Circuit-breaker transition recording logic (snapshot → compare → record) was duplicated between the core `AgentRunner` path (`runner/tool_exec.rs`) and the unified binary runloop (`handlers_batch.rs`), with identical comparison logic in both.                                                                                               | Two independent implementations of the same compare-and-record pattern, using different lock types (sync `parking_lot::Mutex` vs async `tokio::sync::RwLock`).                                             | **Fixed**: extracted `record_circuit_transition_from_snapshot` method on `ErrorRecoveryState` in `error_recovery.rs`. Both paths now call this shared method after acquiring their respective lock.     |
| 14  | **Low**    | `AgentSessionState.executed_commands` grew unboundedly with duplicate tool names — every `push_tool_result` call pushed the tool name without deduplication, so a session with 124 `exec_command` calls stored 124 identical strings.                                                                                                            | `push_tool_result` called `self.executed_commands.push(tool_name.to_owned())` unconditionally. The dead `TrackingState` struct had deduplication logic (`record_command_executed`) but was never wired in. | **Fixed**: `push_tool_result` now checks `contains` before pushing, matching the intended deduplication behavior.                                                                                       |
| 15  | **Low**    | `TrackingState` in `session/tracking_state.rs` was dead code — a parallel tracking abstraction with deduplication methods that was extracted but never wired into `AgentSessionState`. Only its own tests referenced it.                                                                                                                         | Same pattern as the previously removed `loop_detection.rs`: an extraction that was never connected to the live state.                                                                                      | **Removed**: deleted `tracking_state.rs` and its `pub mod` declaration. The deduplication behavior was added directly to `push_tool_result` instead.                                                    |
| 16  | **Low**    | `TurnMetrics` in `session/turn_metrics.rs` was dead code — another parallel abstraction for turn timing/latency tracking, extracted but never wired into `AgentSessionState`. Only its own tests referenced it.                                                                                                                                  | Same dead-extraction pattern as `tracking_state.rs` and `loop_detection.rs`.                                                                                                                               | **Removed**: deleted `turn_metrics.rs` and its `pub mod` declaration.                                                                                                                                   |

### Verification

```text
cargo fmt --all -- --check
cargo check --locked -p vtcode-core -p vtcode
cargo clippy --locked -p vtcode-core -p vtcode --all-targets -- -D warnings
cargo nextest run --profile quick -p vtcode-core -p vtcode
```

All passed: 5,571 tests across `vtcode-core` + `vtcode` (7 fewer from deleted `tracking_state.rs` tests), 46
circuit/recovery tests, 238 session/push_tool tests, clippy clean, formatting clean.

### Files changed

| File                                                                     | Change                                                                                                                                                                       |
| ------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/codegen/vtcode-core/src/core/agent/runner/tool_exec.rs`          | `apply_fallback_success` now strips TUI display fields before `push_tool_result`; `record_circuit_transition` delegates to shared `record_circuit_transition_from_snapshot`. |
| `crates/codegen/vtcode-core/src/core/agent/error_recovery.rs`            | Added `record_circuit_transition_from_snapshot` shared seam; imported `CircuitBreaker`.                                                                                      |
| `crates/codegen/vtcode-core/src/core/agent/session/mod.rs`               | `push_tool_result` deduplicates tool names before pushing; removed dead `pub mod tracking_state`.                                                                            |
| `crates/codegen/vtcode-core/src/core/agent/session/tracking_state.rs`    | Deleted (dead code, never wired into `AgentSessionState`).                                                                                                                   |
| `crates/codegen/vtcode-core/src/core/agent/session/turn_metrics.rs`      | Deleted (dead code, same pattern as `tracking_state.rs`).                                                                                                                    |
| `src/agent/runloop/unified/turn/tool_outcomes/response_content.rs`       | `maybe_inline_spooled_with_preview` skips `tail_preview` when `failure_diagnostics` is present.                                                                              |
| `src/agent/runloop/unified/turn/tool_outcomes/handlers_batch.rs`         | `record_circuit_transition` delegates to shared `record_circuit_transition_from_snapshot`.                                                                                   |
| `src/agent/runloop/unified/turn/tool_outcomes/execution_result/tests.rs` | 1 test for `failure_diagnostics` tail-preview skip.                                                                                                                          |
| `src/agent/runloop/unified/turn/tool_outcomes/response_content.rs`       | `prepare_tool_response_content` strips `view` before any model-facing serialization; TUI reads `view` from the original output via the pipeline-output path.                 |

---

## 2026-08-11 | Continued harness audit: warning-growth guard, ErrorCategory allocation, dead-module cleanup finalization

### Scope

Continuation of the harness core audit. Completed two interrupted improvements from the prior session: (1) a
bounded-warning guard on `AgentSessionState` to prevent unbounded `TaskResults.warnings` growth in long-running
sessions, and (2) eliminating a temporary `String` allocation on the hot tool-failure path by adding
`ErrorCategory::as_str()`.

### Findings and disposition

| #   | Severity   | Finding                                                                                                                                                                                                                                                                                                              | Root cause                                                                                                                                                           | Disposition                                                                                                                                                                                                                                                                                                                                                         |
| --- | ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 17  | **Medium** | `AgentSessionState.warnings` grew unboundedly — every warning site pushed directly to the Vec with no deduplication or cap. In long-running sessions, repeated rate-limit warnings, loop-detector warnings, and budget advisories accumulate indefinitely, bloating `TaskResults.warnings` and the evaluator prompt. | All 11 production call sites used `session_state.warnings.push(...)` directly.                                                                                       | **Fixed**: added `AgentSessionState::push_warning(impl Into<String>)` which (a) skips exact duplicates via `contains`, (b) caps unique warnings at 200, and (c) appends a single elision marker when the cap is hit so data loss is surfaced rather than silent. All 11 production call sites migrated. 3 unit tests added (dedup, distinct retention, cap+marker). |
| 18  | **Low**    | Two hot-path call sites in `runner/tool_exec.rs` called `e.category.to_string()` to get the category label for harness events, allocating a temporary `String` via `Display` on every tool failure.                                                                                                                  | `ErrorCategory` implements `Display` via `user_label()` which returns `&'static str`, but callers used `to_string()` instead of accessing the static slice directly. | **Fixed**: added `ErrorCategory::as_str() -> &'static str` (const, delegates to `user_label()`). Both hot-path call sites changed from `e.category.to_string()` to `e.category.as_str()`. Zero allocation on the failure path.                                                                                                                                      |

### Verification

```text
cargo fmt --all
cargo check --locked -p vtcode-commons -p vtcode-core -p vtcode
cargo nextest run --profile quick -p vtcode-core -E 'test(warning) | test(session) | test(retry) | test(tool_exec) | test(error_category) | test(push_warning)'
cargo nextest run --profile quick -p vtcode-core -p vtcode
cargo clippy --locked -p vtcode-commons -p vtcode-core -p vtcode --all-targets -- -D warnings
```

All passed: 5,569 tests across `vtcode-core` + `vtcode` (3 new `push_warning` tests), 293 focused tests, clippy clean,
formatting clean.

### Files changed

| File                                                                 | Change                                                                                                                          |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `crates/codegen/vtcode-core/src/core/agent/session/mod.rs`           | Added `push_warning` method (dedup + cap + elision marker); `MAX_SESSION_WARNINGS=200`; `WARNINGS_ELIDED_MARKER`; 3 unit tests. |
| `crates/codegen/vtcode-core/src/core/agent/runner/tool_exec.rs`      | 1 call site migrated to `push_warning`; 2 call sites changed from `e.category.to_string()` to `e.category.as_str()`.            |
| `crates/codegen/vtcode-core/src/core/agent/runner/telemetry.rs`      | 2 call sites migrated to `push_warning`.                                                                                        |
| `crates/codegen/vtcode-core/src/core/agent/runner/execute.rs`        | 4 call sites migrated to `push_warning`.                                                                                        |
| `crates/codegen/vtcode-core/src/core/agent/runner/tool_args.rs`      | 2 call sites migrated to `push_warning`.                                                                                        |
| `crates/codegen/vtcode-core/src/core/agent/runner/task_setup.rs`     | 2 call sites migrated to `push_warning`.                                                                                        |
| `crates/codegen/vtcode-core/src/core/agent/runner/tool_rejection.rs` | 1 call site migrated to `push_warning`.                                                                                         |
| `crates/codegen/vtcode-core/src/core/agent/completion.rs`            | 1 call site migrated to `push_warning`.                                                                                         |
| `crates/common/vtcode-commons/src/error_category.rs`                 | Added `as_str() -> &'static str` const method delegating to `user_label()`.                                                     |

---

## 2026-08-01 | Approved-plan recovery and session-runner handoff

Date: 2026-08-01

## Scope

This review covers checkpoint failures in turns `845`–`894`: incomplete plan approval, missing task-tracker handoff,
duplicated session-runner transitions, and silent harness sink failures. Existing pseudo-tool marker recovery, approval
phrase routing, and normalized tracker deduplication were retained.

## Findings and disposition

| Finding                                                                      | Disposition                                                                                                                                                                                                                                            |
| ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Non-empty drafts could reach approval without strict validation.             | `ValidatedPlanArtifact` validates required sections, placeholders, implementation/validation items, assumptions, and unresolved decisions before approval events or overlays. One repair synthesis is allowed; a second rejection remains in planning. |
| Tracker creation errors were swallowed or duplicated across handoff paths.   | `PlanningFinishReason::Approved` creates and verifies the tracker before disabling planning. Missing tools, tool errors, invalid results, and missing tracker files fail closed. All approval routes use `complete_approved_plan_handoff`.             |
| The session runner concentrated lifecycle and recovery wiring in one module. | `session_loop_runner/mod.rs` is a small facade. The orchestration body, harness setup, archive, metrics, plan seed, and support phases have explicit module boundaries.                                                                                |
| Optional harness sinks discarded setup and write failures.                   | Compatibility exports remain best effort and observable through tracing, but canonical `ThreadEvent` persistence now fails closed: setup and drain failures prevent a successful run from being reported.                                              |

## Runtime invariants

- `vtcode-exec-events::ThreadEvent` remains the only runtime event contract.
- Cancellation uses an explicit `Cancelled` finish reason; approval uses `Approved` and cannot disable planning until
  the tracker gate succeeds.
- Direct, queued, automatic, and fresh-context approvals preserve the validated artifact, tracker, execution agent,
  confirmation policy, and context mode in one handoff result.
- Existing authentication, model-picker, configuration, startup, and secrets changes outside this review remain
  untouched.

## Verification

Run the focused planning and session-loop tests, workspace checks, harness checks, agent-legibility check, docs-link
check, and `git diff --check` listed in the root `AGENTS.md`. The targeted compile gate is:

```text
cargo check --locked -p vtcode --bin vtcode
```

---

## 2026-08-11 | Checkpoint cross-check: repeated invalid-arguments loops

Date: 2026-08-11

<!-- Historical or repeated section title retained for existing anchors. -->
<!-- markdownlint-disable-next-line MD024 -->
## Scope

Cross-check of `.vtcode/checkpoints/` (50 JSON files, ~5 MB, turns 860–910) against the harness preflight path. The
checkpoints are independent conversation snapshots; cross-turn tool-result duplication is mostly expected (model
reissues, not harness double execution) and was not treated as a harness defect.

<!-- Historical or repeated section title retained for existing anchors. -->
<!-- markdownlint-disable-next-line MD024 -->
## Findings and disposition

| Finding                                                                                                                                                                                                                                                                      | Severity    | Disposition                                                                                                                                                                                                                                                                                                                                                       |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Repeated `code_search` preflight failures from stringified JSON arguments (`result_types: "[\"path\"]"`, `max_results: "10"`) — six validation failures in a single turn (`turn_864`), continuing into `turn_865`. Each failure + retry wasted several KB and tool attempts. | Medium-High | Added strict, schema-aware string-type coercion in `normalize_schema_aliases_in_place` (`execution_kernel.rs`): a JSON-encoded string is parsed and accepted only when it unambiguously matches the schema-declared top-level type (array/object/integer/number/boolean). Strict `jsonschema` validation still runs afterward, so enum/bounds checks still apply. |
| `max_output_tokens` has a dedicated strict validator that deliberately rejects strings/floats.                                                                                                                                                                               | —           | Excluded from coercion via the shared `MAX_OUTPUT_TOKENS_FIELD` constant so the guardrail is preserved. The contract test confirming string rejection still passes.                                                                                                                                                                                               |
| The `"max_output_tokens"` field name was duplicated as a literal across `output_limits.rs` and `vtcode-utility-tool-specs`.                                                                                                                                                  | Low         | Centralized as `vtcode_utility_tool_specs::MAX_OUTPUT_TOKENS_FIELD`; all four call sites and the schema builder now reference the constant (DRY).                                                                                                                                                                                                                 |

## Coercion boundaries (what it deliberately does NOT do)

- Does not coerce free-form string fields (schema `type` is `string`, an array of types, or absent).
- Does not reinterpret malformed/non-JSON strings — they stay strings and fail strict validation with an actionable
  `describe_jsonschema_error` message.
- Does not coerce floats into integers (`"10.0"` for an integer field stays a string and fails).
- Does not relax enum, `minItems`, `minimum`/`maximum`, or `additionalProperties` checks.
- Does not coerce the output-budget field (`max_output_tokens`).

<!-- Historical or repeated section title retained for existing anchors. -->
<!-- markdownlint-disable-next-line MD024 -->
## Verification

```text
cargo fmt --all -- --check
cargo check --locked -p vtcode-utility-tool-specs -p vtcode-core -p vtcode
cargo clippy --locked -p vtcode-utility-tool-specs -p vtcode-core -p vtcode --all-targets -- -D warnings
cargo nextest run --profile quick -p vtcode-core -p vtcode
```

Result: 5,579/5,579 tests passed, 12 skipped; format and clippy clean. New focused tests cover
array/integer/boolean/object/number coercion, float-not-integer rejection, mismatched/malformed rejection, `code_search`
end-to-end coercion, post-coercion bounds/enum enforcement, and `max_output_tokens` strict-rejection preservation.

---

## 2026-08-11 | Preflight path KISS/DRY consolidation

Date: 2026-08-11

<!-- Historical or repeated section title retained for existing anchors. -->
<!-- markdownlint-disable-next-line MD024 -->
## Scope

Follow-up audit of the preflight validation path (`execution_kernel.rs`) and serial tool-execution path
(`runner/tool_exec.rs`) for duplicated logic, redundant work, and tight coupling. Evidence-driven: only patterns with
concrete duplication or measurable wasted work were changed.

<!-- Historical or repeated section title retained for existing anchors. -->
<!-- markdownlint-disable-next-line MD024 -->
## Findings and disposition

| Finding                                                                                                                                                                                                                          | Severity                 | Disposition                                                                                                                                                                                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `preflight_validate_resolved_call` fetched the same tool's parameter schema up to 3× per call (initial normalize, remap branch, and a third lookup before jsonschema validation). Each lookup clones a `Value` schema.           | Medium (perf)            | Bound `effective_parameter_schema` once; remap branches overwrite it with the schema they already fetch; the third lookup was removed entirely. The no-remap path (the common case) now does 1 lookup instead of 2; the file-remap path does 2 instead of 3.                                               |
| The 5-key path-alias list `["path","file_path","filepath","target_path","file"]` was duplicated across 4 sites: `is_missing_required_arg`, the preflight path-safety `or_else` chain, and two `execution_history.rs` extractors. | Medium (DRY/correctness) | Extracted `pub(super) const PATH_ALIAS_KEYS` in `execution_kernel.rs`; all 4 sites now reference it so the key set and order cannot drift. The `safety_gateway` (4 keys, no "file") and `tool_intent/actions` (6 keys, adds "p") variants were intentionally left alone — their key sets differ by design. |
| The `format!("Missing required argument: {key}")` was inlined in two places in `preflight_validate_resolved_call`.                                                                                                               | Low (DRY)                | Extracted `missing_required_arg_failure(key)` as the single source for the failure wording.                                                                                                                                                                                                                |
| The serial tool-execution path (`tool_exec.rs`) duplicated the identical 4-line latency push in both match arms; the parallel path already used the cleaner hoist-once pattern.                                                  | Low (DRY)                | Hoisted the latency measurement+push to run once before the match, mirroring the parallel path. Semantics unchanged (the push was first in both arms).                                                                                                                                                     |
| The two action-alias remappers (`remap_public_file_operation_alias_args`, `remap_consolidated_action_alias_args`) duplicated the `as_object? + contains_key("action")` guard and the `clone + insert("action") + wrap` tail.     | Low (DRY)                | Extracted `args_object_without_action` and `with_action_inserted` helpers; each remapper is now just its action-mapping table.                                                                                                                                                                             |

## Module coupling assessment (step 5)

`preflight_validate_resolved_call` is ~140 lines with 5 co-evolving mutable variables across the remap branches. This is
the natural coupling point, but it is linear (each phase follows the previous) and the duplication removals above
eliminated the drift risk. Splitting it further would thread 5 variables through helper signatures for marginal
testability gain and was deliberately NOT done — consistent with the constraint that splitting requires evidence of a
concrete defect, not just length. The new pure helper `parse_string_as_schema_type` is the independently-testable
isolation chunk for the coercion logic.

<!-- Historical or repeated section title retained for existing anchors. -->
<!-- markdownlint-disable-next-line MD024 -->
## Verification

```text
cargo fmt --all -- --check
cargo check --locked -p vtcode-utility-tool-specs -p vtcode-core -p vtcode
cargo clippy --locked -p vtcode-utility-tool-specs -p vtcode-core -p vtcode --all-targets -- -D warnings
cargo nextest run --profile quick -p vtcode-core -p vtcode
```

Result: 5,579/5,579 tests passed, 12 skipped; format and clippy clean. No new tests required for the pure DRY refactors
(behavior preserved); the existing 640 preflight/remap/alias/code_search tests cover the changed paths.

---

## 2026-08-11 | Canonical persistence and harness stability follow-up

### Scope

Revisited the sampled 3,253-event session and the harness persistence paths after the token/context fixes. The
implementation now treats the workspace `vtcode-memory` session store as authoritative for both interactive and exec
runs.

### Findings and disposition

| Finding                                                                                         | Severity | Disposition                                                                                                                                                                                                                                                                                                                                                                               |
| ----------------------------------------------------------------------------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Synchronous JSON serialization, file writes, and `flush()` ran on every harness event.          | High     | Canonical events now use one bounded non-blocking handoff with event-count and estimated-byte limits to a blocking drain; saturation fails closed instead of growing memory or silently dropping authoritative events. Explicit legacy, Open Responses, and ATIF exporters are isolated; JSONL exporters use `AsyncLineWriter`, while ATIF serialization and writes use `spawn_blocking`. |
| Interactive harness logs and exec logs used a global default path in addition to session state. | High     | Canonical events are stored at `<workspace>/.vtcode/sessions/<session_id>/events.jsonl`. `agent.harness.event_log_path` and exec `--events` are explicit compatibility exports; unset configuration creates no global harness file.                                                                                                                                                       |
| Retention trusted `manifest.json.session_id` when constructing deletion paths.                  | High     | Retention enumerates direct child directories, skips symlinks and active sessions, and deletes only validated children of `.vtcode/sessions`. Manifest IDs cannot redirect deletion.                                                                                                                                                                                                      |
| Independent exporter locks allowed consumers to observe divergent ordering.                     | Medium   | One dispatch gate feeds canonical, legacy, Open Responses, and ATIF consumers in the same order.                                                                                                                                                                                                                                                                                          |
| Millisecond-only harness IDs collided in the sampled logs.                                      | Medium   | Harness-generated item IDs use UUIDs; the 3,253-event regression test asserts uniqueness.                                                                                                                                                                                                                                                                                                 |
| Session status became terminal at turn boundaries.                                              | Medium   | `thread.started`/`turn.started` establish activity; only `thread.completed` makes the manifest terminal.                                                                                                                                                                                                                                                                                  |
| The harness facade mixed persistence, conversion, retention, and construction.                  | Low      | Canonical, legacy, Open Responses, ATIF, and path helpers are now sibling modules behind a thin facade.                                                                                                                                                                                                                                                                                   |

### Persistence invariants

- Canonical store open and drain failures are propagated; authoritative event loss cannot be reported as a successful
  run.
- Canonical events retain their existing per-session event cap. Retention keeps the 50-session/30-day defaults and
  leaves historical global artifacts alone.
- Optional exporter drops are reported separately and never redefine the canonical event contract (`ThreadEvent`).
- Native Codex exec emits lifecycle-only canonical events because it does not have VT Code tool-event data available; it
  does not synthesize tool events.

### Post-implementation adversarial review

The second review found no Critical or High findings. The remaining Medium finding was concurrent finalization: a second
caller could previously observe an empty handle and return before the first canonical drain completed. The canonical
sink and harness facade now share one-shot completion results, so concurrent `close`/`finish` callers all wait for the
same outcome. Unexpected shutdowns also log terminal-event enqueue failures before reporting drain failures. No
actionable Medium findings remain.

### Verification

```text
cargo nextest run -p vtcode-memory
cargo nextest run -p vtcode-core -E 'test(/session_store_sink|event/)'
cargo nextest run -p vtcode -E 'test(/harness/)'
cargo fmt --all -- --check
```

The focused memory, sink, harness, ordering, retention, lifecycle, and large-batch tests pass.
