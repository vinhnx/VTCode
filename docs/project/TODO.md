implement:

````
# VTCode Modes & Subagents Audit

**Date:** 2026-06-21
**Scope:** `plan`, `build`, and `auto` modes, the subagent system, and the orchestration glue between them.
**Mode:** Read-only audit. No source files were modified.

## Executive Summary

The mode/subagent system has matured significantly (config in `vtcode-config/src/subagents.rs`, runtime in `vtcode-core/src/subagents/`, orchestration in `src/agent/runloop/unified/`), but there is a coherent set of **blockers**, **technical issues**, and **workflow inefficiencies** that primarily stem from the asymmetric treatment of `auto` versus the other primaries and the fragmented planning-workflow state. The single most impactful class of bugs is *silent fallback* — both the resolution and the planning handoff can drop user intent without warning.

Top three blockers:

1. `auto` full-auto path silently downgrades to `build` when `default_primary_agent` is set to anything other than `auto` (e.g., `duck`).
2. Interactive `--auto` and `codex_app_server` full-auto sessions bypass the `WorkspaceTrustLevel::FullAuto` trust gate that protects the non-interactive path.
3. Planning workflow state is duplicated across four locations; the codex bridge and unified runloop can disagree on whether planning is active.

---

## 1. Mode Map (plan / build / auto / duck)

Built-in primaries are defined together in `vtcode-config/src/subagents.rs:469-472` via `builtin_subagents()`. Resolution happens in `src/agent/runloop/unified/session_setup/init.rs:455` (`active_primary_agent_from_specs_for_mode`).

| Mode | `mode` field | Default perms | Tools | Notes |
|---|---|---|---|---|
| `build` (line 556) | `Primary` | `Ask` | full + `unified_exec` policy override | `DEFAULT_PRIMARY_AGENT_NAME` |
| `auto` (line 587) | `Primary` | `Auto` | full + `unified_exec` policy override | Hard-required by full-auto path |
| `duck` (line 649) | `Primary` | `Deny` + `allow=["read"]` | read-only + `request_user_input` | discussion-first |
| `plan` (line 620) | **`All`** | `Deny` + `allow=["read"]` | read-only + `request_user_input` | works as both subagent and primary |

`DEFAULT_PRIMARY_AGENT_NAME = "build"` (`vtcode-config/src/constants/defaults.rs:17`).

---

## 2. Blocker-Level Issues

### 2.1 Asymmetric defaulting semantics between auto and others
**File:** `src/agent/runloop/unified/session_setup/init.rs:455-475`

`active_primary_agent_from_specs_for_mode` has two distinct branches:

- `full_auto && !primary_agent_explicitly_configured` → `select_from_specs("auto")` with `bail!` on miss
- otherwise → `from_specs_with_default`, which **silently falls back to `build`** if the requested agent is unknown

The test `explicit_duck_is_honoured_for_full_auto` (`src/cli/full_auto_primary_agent.rs:91`) documents this silently: configuring `default_primary_agent="duck"` for full-auto yields `build` with no user warning. This is a **UX and safety blocker** — a user who types `--auto` after setting `default_primary_agent="duck"` runs a build agent under auto trust.

**Fix:** surface a stderr warning, or expand the fail-fast condition to any spec miss under full-auto.

### 2.2 Missing trust gate for interactive `--auto` runs
**File:** `src/cli/auto.rs:32` vs `src/startup/workspace_trust.rs:84`

`require_full_auto_workspace_trust` is called only on the **non-interactive** `--auto` path. The `vtcode ask` / `codex_app_server` interactive runloop (`src/codex_app_server/runtime.rs:184-198`) accepts `full_auto=true` without invoking the trust gate. A user can flip into auto trust semantics without ever re-prompting for trust.

### 2.3 `plan` is registered as `AgentMode::All` while its siblings are `Primary`
**File:** `vtcode-config/src/subagents.rs:620,649`

`builtin_plan_agent.mode = AgentMode::All` (default `Subagent`), so the `plan` primary is simultaneously exposed to the subagent registry. This causes:

- `effective_specs()` (`vtcode-core/src/subagents/mod.rs:177`) surfaces `plan` as both a primary and a subagent, confusing delegation.
- `select_from_specs` is called with hard-coded strings `"auto"` and `"build"`, but `"plan"` is resolved through `from_specs_with_default` (or not at all from `full_auto_primary_agent.rs`).

**Fix:** treat `plan` like `duck` (`Primary` only) and provide a separate `plan` subagent, or keep dual mode but document the discovery filter.

### 2.4 Lifecycle tool policy hole in `DeferredToolPolicy`
**File:** `vtcode-core/src/tools/handlers/session_tool_catalog.rs:57-130`

`DeferredToolPolicy.keeps_entry_available` only matches `public_name` / `registration_name` / `aliases` (line 87-100). Multi-action tools like `unified_exec` (which subsumes build/test/eval actions) cannot have a subset of actions made available while others are deferred. This forces all-or-nothing availability of compound tools when the Anthropic/OpenAI tool-search deferred mode is enabled.

### 2.5 Dual trust enums with no shared vocabulary
**Files:** `vtcode-config/src/acp.rs:176` (`WorkspaceTrustLevel`: ToolsPolicy/FullAuto) and `vtcode-core/src/tools/safety_gateway.rs:44` (`SafetyTrustLevel`: Untrusted/Standard/Elevated/Full)

These exist in different crates, encode overlapping concepts, and are bridged ad-hoc inside slash commands (`src/agent/runloop/unified/turn/session/slash_commands/planning.rs:80-99` calls `should_enforce_safe_mode_prompts` which re-derives safety level from ACP trust plus the `full_auto` flag). Any new trust tier must be threaded through both enums, the ACP config schema, and the bridge.

---

## 3. Technical Issues

### 3.1 Planning workflow state lives in 3+ places
- `src/agent/runloop/unified/planning_workflow_state.rs` — session state (turns, interview cycles)
- `vtcode-core/src/tools/registry/planning_workflow_facade.rs` — `ToolRegistry.enable_planning()` / `is_planning_active()`
- `vtcode-core/src/tools/handlers/planning_workflow.rs` (handlers) — `PlanningWorkflowState` (the flag)
- `src/codex_app_server/runtime.rs:415-432` — local `planning_active: bool` for the codex bridge

The `is_planning_active()` accessor in `planning_workflow_facade.rs:43` documents itself as the single source of truth, but the `codex_app_server` path keeps a parallel `bool` and synchronises via `normalize_planning_input` (line 999) on the codex side and `transition_to_planning_workflow` (line 112) on the unified runloop side. These never exchange state, so a slash command toggling planning on the unified runloop will not be observed by the codex bridge (and vice versa).

### 3.2 `normalize_planning_input` vs `transition_to_planning_workflow` handoff drift
**File:** `src/codex_app_server/runtime.rs:999-1018`

`is_planning_active_implementation_alias` and `should_switch_to_execution_mode` define **slightly different** alias sets:

- implementation: `"implement" | "continue" | "go" | "start" | "yes"`
- switch: `"implement" | "continue" | "go" | "start" | "yes" | "exit planning workflow"`

The implementation set never includes `"exit planning workflow"`, so typing that exact phrase on a codex interactive session leaves `planning_active=true` while the unified runloop has `PlanLifecyclePhase::ActiveDrafting`. End result: the UI says you are executing; the prompt still routes through the planning system prompt.

### 3.3 Tool catalog invalidation hook is correct but undocumented
**File:** `vtcode-core/src/tools/registry/planning_workflow_facade.rs:17-40`

`enable_planning()` and `disable_planning()` correctly call `note_explicit_refresh("planning_workflow_enabled" / "planning_workflow_disabled")`. However, `transition_to_planning_workflow` (`src/agent/runloop/unified/planning_workflow_state.rs:112`) calls `tool_registry.enable_planning()` AND `plan_state.enable()` separately — the second call is the one whose `note_explicit_refresh` invalidates the cache. If a future refactor moves one of these two calls, the cache will silently stale. There is no test covering the `enable()` short-circuit (line 19) when `was_active` is already true.

### 3.4 `note_explicit_refresh` is correct but consider the same pattern for `set_phase`
`PlanningWorkflowState::set_phase(PlanLifecyclePhase::ActiveDrafting)` (`src/agent/runloop/unified/planning_workflow_state.rs:114`) does not invalidate the catalog, even though tool filtering reads phase (e.g., `vtcode-core/src/tools/registry/planning_workflow_checks.rs`). A `Drafting -> FinalReview` transition with a tool policy change can leave a stale snapshot.

### 3.5 `is_subagent_cleanup_tool` hard-codes two tool names
**File:** `vtcode-core/src/primary_agent.rs:192`

```rust
fn is_subagent_cleanup_tool(tool_name: &str) -> bool {
    tool_name == tools::WAIT_AGENT || tool_name == tools::CLOSE_AGENT
}
````

If a new lifecycle tool is added (e.g. `list_agents`, `pause_agent`), it must be edited here in addition to the `tools::` constant block. Centralise in the constant list.

### 3.6 Mode resolution ignores `default_primary_agent` for the full-auto path

**File:** `src/agent/runloop/unified/session_setup/init.rs:457-465`

When `full_auto=true` AND `primary_agent_explicitly_configured=true`, the resolver still calls `select_from_specs("auto")`. But the user's `vtcode.toml` may have `default_primary_agent="auto"` — they will be told the build agent is active because of the precedence rules. The `primary_agent_explicitly_configured` flag is set in only one place; the relationship to the `default_primary_agent` field is undocumented and easy to break.

### 3.7 Agent name resolution precedence is in `resolve_primary_agent`

**File:** `vtcode-core/src/primary_agent.rs:172-191`

The first match wins on `name.eq_ignore_ascii_case`, then alias. With `builtin_primary_build_agent` having aliases `["builder"]` and a user project defining `builder.md` with `mode: "primary"`, the **builtin always wins** because the loop picks the first spec with a name match before consulting aliases. There is a test for the inverse (`alias_resolution_uses_existing_matches_name_semantics`) but no test that documents project agents outranking builtin aliases.

### 3.8 `WorkspaceTrustLevel` has no `Ask` tier

**File:** `vtcode-config/src/acp.rs:176`

Only `ToolsPolicy` (default) and `FullAuto`. There is no representation of "ask-for-trust-each-session" or "ask once then remember for N days." A finer-grained trust model would be required for headless CI or ephemeral sandboxes.

---

## 4. Workflow Inefficiencies

### 4.1 `builtin_subagents` reallocates four full `SubagentSpec`s on every discovery call

**File:** `vtcode-config/src/subagents.rs:466-475`

`discover_subagents` calls `builtin_subagents()` (which constructs `builtin_primary_build_agent()`, `builtin_primary_auto_agent()`, `builtin_primary_duck_agent()`, `builtin_plan_agent()`, plus the rest) on **every** call. Each agent has `BTreeMap` policy overrides and a clone of the long prompt string. `SubagentController::new` (`vtcode-core/src/subagents/mod.rs:115`) and `discover_subagents` (line 395) both call it independently, doubling the work. Consider `OnceCell<Vec<SubagentSpec>>` or a builder pattern.

### 4.2 `effective_specs()` clones the entire spec vector on every call

**File:** `vtcode-core/src/subagents/mod.rs:177`

`effective_specs()` returns `Vec<SubagentSpec>` (a clone of the full list), and is called in hot paths (`init.rs:308`, `init.rs:317`). For workspaces with many user-defined subagents, this is several MB of `String` allocation per call. Consider returning `Arc<[SubagentSpec]>`.

### 4.3 Shadowed specs are discarded

**File:** `vtcode-config/src/subagents.rs:451-456`

`discover_subagents` records shadowed specs but returns them only in the `DiscoveredSubagents` struct, which the caller (`SubagentController::new`, line 124) ignores via `discovered.effective`. Project `.claude/agents/foo.md` overriding `~/.vtcode/agents/foo.md` is invisible to operators. Expose a CLI flag to surface shadowed specs.

### 4.4 The build default is silent — no log when fallback fires

**File:** `vtcode-core/src/primary_agent.rs:113-126`

`from_specs_with_default` returns the builtin `build` spec on miss, with no `tracing` event, no `eprintln`, no warning. Combined with §2.1, this is the most common silent-degradation path in the system.

### 4.5 Slash-command `/plan` and `--plan` route to different code paths

- `--plan` (CLI flag) eventually flows through `codex_app_server/runtime.rs:184` (sets `planning_active=true` in the codex bridge).
- `/plan` (slash command) flows through `src/agent/runloop/unified/turn/slash_commands/planning.rs:18` which calls `transition_to_planning_workflow` (sets the unified runloop state).

These two paths don't share state, so a user starting in codex then typing `/plan` flips the unified-runloop flag but leaves the codex bridge's `planning_active` stale.

### 4.6 `should_attempt_dynamic_interview_generation` is mentioned in summary but not in tests

**File:** `src/agent/runloop/unified/turn/turn_processing/planning_workflow.rs` (TD-005 hotspot)

The summary flags interview synthesis as a known hotspot. The 20s timeout in `synthesize_planning_workflow_interview_args` is a fallback, but the function is `async` and runs on the same turn loop — a slow LLM round-trip will block the entire turn. Suggest moving to a `tokio::spawn` with a `oneshot` for the interview result.

### 4.7 `apply_patch` tool policy override is missing for read-only primaries

**File:** `vtcode-config/src/subagents.rs:566-575, 532-543, 660-670`

`builtin_primary_build_agent` and `builtin_primary_auto_agent` only add `unified_exec -> Allow`. But `builtin_plan_agent` and `builtin_primary_duck_agent` have **no** `tool_policy_overrides` and rely entirely on the `tools` allow-list. If a user has set `[tools.policies] apply_patch = "prompt"` globally, the plan/duck agents will still respect that prompt. The intent of "blocked" for read-only primaries is undermined by the global policy. Add explicit `apply_patch -> Deny` overrides.

### 4.8 `MAX_PERMISSION_HOOK_UPDATES = 64` and `MAX_PERMISSION_UPDATE_RULES = 128` are top-of-file constants

**File:** `src/agent/runloop/unified/tool_routing/mod.rs`

These limits are good but they are only enforced in routing; the `update_approval_cache!` and `update_permission!` tools may bypass them via direct registry writes. Document the boundary.

---

## 5. Known Review Findings (from `review-findings.json`)

| Severity | File                                                    | Issue                                                                            |
| -------- | ------------------------------------------------------- | -------------------------------------------------------------------------------- |
| high     | `src/agent/runloop/unified/state.rs:554`                | Doc diagram omits 200ms debounce sub-window — misleading for cancellation tests  |
| medium   | `src/agent/runloop/unified/async_mcp_manager.rs:143`    | Comment claims `spawn_blocking` but code uses `tokio::spawn`; dropped JoinHandle |
| medium   | `src/agent/runloop/tool_output/files.rs:15`             | `render_tree_detail()` duplicates `tool_summary.rs:249-256`                      |
| low      | `src/agent/runloop/unified/stop_requests.rs:35`         | `notify_one()` may delay cancellation in batch tool execution                    |
| low      | `src/agent/runloop/unified/state.rs:1139`               | Thread-safety test assertion is too weak                                         |
| low      | `src/agent/runloop/unified/tool_summary_helpers.rs:349` | Stale "Pagination / read plumbing" comment                                       |

These are independent of the mode/subagent audit but several (state.rs, stop_requests.rs) sit on hot paths used by `auto` mode's stop handling.

---

## 6. Recommended Remediation Order

| #   | Action                                                                                                                 | Impact             | Effort  |
| --- | ---------------------------------------------------------------------------------------------------------------------- | ------------------ | ------- |
| 1   | Add `tracing::warn!` (or stderr) when `from_specs_with_default` falls back to `build`                                  | High (UX safety)   | Small   |
| 2   | Make full-auto path honour `default_primary_agent` OR surface explicit warning when configured agent is dropped        | High               | Small   |
| 3   | Extend trust-gate enforcement to interactive `--auto` and codex full-auto path                                         | High (safety)      | Medium  |
| 4   | Centralise `PlanningWorkflowState` as the single source of truth (delete `codex_app_server` local `bool`)              | High (consistency) | Medium  |
| 5   | Reconcile `is_planning_active_implementation_alias` and `should_switch_to_execution_mode` into one canonical alias set | Medium             | Small   |
| 6   | Change `builtin_plan_agent.mode` from `All` to `Primary` (provide separate subagent)                                   | Medium             | Small   |
| 7   | Add explicit `apply_patch -> Deny` override to `plan` and `duck` primaries                                             | Medium             | Small   |
| 8   | Replace per-call `builtin_subagents()` allocation with `OnceCell`                                                      | Medium (perf)      | Small   |
| 9   | Make `effective_specs()` return `Arc<[SubagentSpec]>`                                                                  | Low (perf)         | Small   |
| 10  | Unify `WorkspaceTrustLevel` + `SafetyTrustLevel` behind a single canonical `TrustTier` enum                            | Medium (long-term) | Large   |
| 11  | Apply `note_explicit_refresh` to `set_phase` in addition to `enable`/`disable`                                         | Low                | Small   |
| 12  | Add lifecycle-tool constant list instead of hard-coding in `is_subagent_cleanup_tool`                                  | Low                | Trivial |

---

## 7. Verification Hooks Already in Place

- `src/cli/full_auto_primary_agent.rs:81` — `defaulted_full_auto_uses_effective_custom_auto`
- `src/cli/full_auto_primary_agent.rs:91` — `explicit_duck_is_honoured_for_full_auto` (documents the silent-fallback)
- `src/cli/full_auto_primary_agent.rs:117` — `missing_defaulted_auto_fails_fast`
- `vtcode-core/src/primary_agent.rs:567-625` — extensive resolution tests
- `vtcode-core/src/subagents/mod.rs:3240-3340` — `spawn_background_subprocess` lifecycle tests
- `src/codex_app_server/runtime.rs:1164-1185` — `normalize_planning_input` tests

These should be extended to cover the asymmetry and codex-bridge inconsistency noted above.

---

## 8. Files Reviewed (read-only)

- `vtcode-config/src/subagents.rs` (built-in definitions, discovery, naming)
- `vtcode-config/src/acp.rs` (WorkspaceTrustLevel)
- `vtcode-core/src/primary_agent.rs` (resolution, fallbacks)
- `vtcode-core/src/subagents/{mod,discovery,model,types}.rs` (controller)
- `vtcode-core/src/tools/registry/{mod,planning_workflow_facade,planning_workflow_checks}.rs`
- `vtcode-core/src/tools/handlers/session_tool_catalog.rs` (DeferredToolPolicy)
- `vtcode-core/src/tools/safety_gateway.rs` (SafetyTrustLevel)
- `src/agent/runloop/unified/planning_workflow_state.rs` (session state)
- `src/agent/runloop/unified/turn/turn_processing/planning_workflow.rs` (interview/turn)
- `src/agent/runloop/unified/turn/session/slash_commands/planning.rs` (slash handler)
- `src/agent/runloop/unified/session_setup/init.rs` (resolver)
- `src/agent/runloop/unified/tool_routing/{mod,shell_approval}.rs` (already under user modification; not touched)
- `src/cli/{auto,full_auto_primary_agent}.rs` (CLI auto)
- `src/codex_app_server/runtime.rs` (codex bridge, planning handoff)
- `src/startup/workspace_trust.rs` (trust gate)
- `review-findings.json`, `.vtcode/plans/vtcode-modes-audit*.md` (existing plan)

No files were modified. Ready to begin remediation under explicit task delegation if desired.

```

```
