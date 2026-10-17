---
feature: budget-retune-dead-tools
status: delivered
updated: 2026-09-27
branch: feat/budget-retune-dead-tools
commits: 12a631c1d..e2dfe4e86
---

# Budget Retune and Dead Tool IDs

## Report

**What was built** — Progressive tool-schema budget raised from 1,800 to **2,000** (~10% headroom over the measured
1,793 on the Codex-4 default profile) so small description tweaks no longer trip the gate. First-request ceilings stay
6k/8k. Audit of `BUILTIN_TOOLS` found **no dead registered tools** (`get_errors` remains LLM-hidden for back-compat);
four unused `*_ID` constants in `tools/mod.rs` were deleted. `ToolDocumentationMode::Progressive` docs now distinguish
measured size vs budgeted cap.

**Verification** — `./scripts/check-dev.sh` PASS. `cargo nextest run -p vtcode-core -p vtcode --no-fail-fast` **7364
passed, 1 failed, 12 skipped**; failure is **PRE-EXISTING** `cli_harness_failures::print_mode_requires_prompt_or_stdin`
(auth env). Targeted: schema budget, first-request budget, `get_errors` back-compat — PASS. Independent review:
**approve**.

### Journey log

- "Remove dead tools if needed" audit outcome is often "none registered are dead" — record that explicitly rather than
  inventing removals.
- Progressive schema budget has two doc surfaces: EXECUTION_POLICY (the gate) and the `ToolDocumentationMode` doc
  comment (the measurement). Reconcile both when the cap moves.
- Historical compose-spec reports must not be retro-edited when budgets change.

## [S1] Problem

After `lean-harness-defaults`, the Progressive tool-schema gate sits at **1,793 / 1,800** — only 7 tokens of headroom.
Any small description tweak trips `emitted_model_tool_schema_fits_within_first_request_budget` immediately. Separately,
the follow-up scope allowed removing tools from the registry _if needed_ and re-tuning budgets after the lean change.

An audit of `BUILTIN_TOOLS` registrations (`pack_impls.rs` + `builtins.rs`) found **no dead registered tools**: every
registration has a live executor or an explicit back-compat contract (`get_errors` is LLM-hidden and tested for
compatibility). Four `*_ID` string constants in `tools/mod.rs` (`CREATE_APPLY_PATCH_FREEFORM_TOOL_ID`,
`CREATE_APPLY_PATCH_JSON_TOOL_ID`, `INTERCEPT_APPLY_PATCH_ID`, `NEW_SHARED_TRACKER_ID`) have zero references
workspace-wide and are dead code.

## [S2] Design

Settled (grill 2026-09-27): re-tune schema cap to **2,000** (keep first-request 6k/8k); remove only dead code — no
registry removals unless the audit finds truly unused registrations.

### S2-budget: Progressive schema cap 1800 → 2000

- Change `emitted_model_tool_schema_fits_within_first_request_budget` assertion from `<= 1_800` to `<= 2_000`.
- First-request ceilings stay **6,000** (no MCP) and **8,000** (MCP); Default-mode ≤6,000 assertion unchanged.
- Update `docs/development/EXECUTION_POLICY.md` budget table and the 1,800-token envelope mention to 2,000. Note the
  measured value (~1,793) and intended ~10% headroom.
- Do not change tool descriptions or `PROGRESSIVE_DESCRIPTION_MAX_CHARS` in this change.

### S2-dead: remove unused tool ID constants

- Delete the four unused constants in `crates/codegen/vtcode-core/src/tools/mod.rs` listed in S1.
- Leave all `BUILTIN_TOOLS` registrations in place (audit: none are dead).
- Do not touch `is_removed_public_tool_name` / `is_removed_default_public_tool` lists.

## [S3] Out of Scope

- Removing any registered tool (audit found none dead; `get_errors` stays for back-compat).
- Changing first-request 6k/8k ceilings or Minimal default.
- Tool description trims or `PROGRESSIVE_DESCRIPTION_MAX_CHARS`.
- Prompt-prose or tool-surface changes.

## Tasks

- [x] T1: schema budget 2,000 + docs — acceptance: `emitted_model_tool_schema_fits_within_first_request_budget` asserts
      `<= 2_000` and passes; EXECUTION_POLICY numbers say 2,000 (covers: S2-budget)
- [x] T2: drop unused tool ID constants — acceptance: `CREATE_APPLY_PATCH_FREEFORM_TOOL_ID`,
      `CREATE_APPLY_PATCH_JSON_TOOL_ID`, `INTERCEPT_APPLY_PATCH_ID`, `NEW_SHARED_TRACKER_ID` no longer exist;
      `cargo check` / clippy clean; no other references (covers: S2-dead)
