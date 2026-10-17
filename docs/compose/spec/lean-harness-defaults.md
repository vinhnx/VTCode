---
feature: lean-harness-defaults
status: delivered
updated: 2026-09-27
branch: feat/lean-harness-defaults
commits: 4f8fe0105..08d4682d9
---

# Lean Harness Defaults

## Report

**What was built** — VT Code's out-of-the-box defaults are leaner, following HarnessTax's cost-first findings.
`SystemPromptMode::Minimal` (~500 tokens) is now the enum default; missing-config fallbacks (including
`fallback_base_system_prompt`) resolve through `SystemPromptMode::default()` so they never silently pick the fuller
Default profile. Progressive tool-schema budget is **1,800** tokens (measured 1,793) and first-request ceilings are
**6,000** (no MCP) / **8,000** (MCP), measured against the effective-default prompt. The always-eager tool surface is
the Codex baseline only (`exec_command`, `write_stdin`, `apply_patch` when supported, `search_tools`);
`task_tracker`/`start_planning` stay eager while planning is active, and
`agent`/`list_skills`/`load_skill`/`load_skill_resource` defer until `search_tools` surfaces them.

**Verification** — `./scripts/check-dev.sh` PASS. `cargo nextest run --no-fail-fast` **8641 passed, 1 failed, 12
skipped**; the failure is **PRE-EXISTING** `cli_harness_failures::print_mode_requires_prompt_or_stdin` (auth env,
documented in gotchas). Targeted gates: schema ≤1800, first-request 6k/8k, Default-mode ≤6k assertion, golden prompts
pinned, `lean_defaults_defer_planner_and_skills_tools_outside_planning` — all PASS. First review: **request-changes**
(critical: `fallback_base_system_prompt(None)` still returned Default text). Fix `08d4682d9` + re-review: **approve**.

### Journey log

- Spec's "~1,844" schema measurement was stale — actual was already 1,793, so no description trims were required;
  headroom is only 7 tokens against the 1,800 cap.
- `fallback_base_system_prompt` is a second missing-config default site (binary seed path) separate from `vtcode-core`'s
  `unwrap_or_default()` sites — `_ => default_system_prompt()` catch-alls must go when the enum default changes.
- Default-mode goldens/budget tests that used `VTCodeConfig::default()` without pinning `system_prompt_mode` silently
  flipped to Minimal; pin them explicitly when the enum default moves.
- Drive-by: cleared PRE-EXISTING `unnecessary_literal_unwrap` clippy in plan-mode tests / plan-approval so check-dev
  could pass on this base.
- `emitted_model_tool_schema_fits_within_first_request_budget` measures the default `ToolProfile::VtCode` visible set
  (Codex-4), not the whole registry.

## [S1] Problem

HarnessTax (Pan et al.) shows harness fixed overhead — instructions + tool schemas on the first call — can drive up to
5x cost differences at the same success rate. `harness-tax-observability` made that tax measurable; this change **lowers
the tax** by making VT Code's out-of-the-box defaults leaner:

1. `SystemPromptMode` default is `Default` (~900 tokens base). `Minimal` (~500, Pi-inspired) is opt-in. Every session
   pays the larger prompt unless the user knows to flip it.
2. Progressive tool-schema cap is 2,200 tokens and the combined first-request cap is 12,000 (15,000 with MCP) — far
   above what is actually sent with a minimal prompt. The caps do not stop slow prompt/schema bloat.
3. `is_core_tool_entry` keeps `TASK_TRACKER`, `START_PLANNING`, `AGENT`, `LIST_SKILLS`, `LOAD_SKILL`,
   `LOAD_SKILL_RESOURCE` never-deferred alongside the Codex baseline. Those ride on every request even when unused.

## [S2] Design

Settled decisions (grill 2026-09-27): Minimal default; budgets 1800 / 6k / 8k; Codex-4 eager only. Direction is leaner
(cost-first), matching HarnessTax.

### S2-prompt: SystemPromptMode::Minimal is the default

- In `vtcode-config`, move `#[default]` from `Default` to `Minimal` on `SystemPromptMode`.
- Keep all four modes parseable/configurable; `default`/`specialized` remain explicit opt-ins.
- Config fallbacks that currently `unwrap_or(SystemPromptMode::Default)` (and match-based equivalents such as
  `fallback_base_system_prompt`) switch to `unwrap_or_default()` / exhaustive match so missing config follows the new
  default.
- `prompts::static_prompts::default_system_prompt()` continues to return the **Default-mode** prompt text (the name
  means "the `Default` profile", not "the configured default"). Composed-prompt paths already select by mode. Do **not**
  rename it; tests that assert Default-profile content keep working.
- First-request budget tests measure the **effective default** (`SystemPromptMode::default()` → Minimal) plus a separate
  assertion that the Default-mode profile still fits the schema/first-request budget when selected explicitly.
- Subagent lightweight profile already forces Minimal; unchanged.
- Startup token-overhead warning text no longer claims lightweight is the default.

### S2-budgets: tighter Progressive / first-request caps

In `tools/registry/builtins.rs` budget tests (and mirrored docs):

| Budget                                  | Before   | After       |
| --------------------------------------- | -------- | ----------- |
| Progressive builtin tool-schema tokens  | ≤ 2,200  | ≤ **1,800** |
| First request (no MCP)                  | ≤ 12,000 | ≤ **6,000** |
| First request (with MCP growth ceiling) | ≤ 15,000 | ≤ **8,000** |

- Progressive descriptions currently measure 1,793 tokens. Trim description tails only if the measured total exceeds
  1,800; the test is the gate. Do not cut the verb/constraint cues required by the tool-description contract.
- First-request composition for the gate = effective-default system prompt + tool schemas + instruction appendix (250) +
  welcome addendum (200) heuristics, same formula as today.
- MCP growth ceiling expressed as 8,000.

### S2-tools: Codex-4 always-eager

`is_core_tool_entry` always-eager set becomes the Codex baseline only:

- `exec_command`, `write_stdin`, `search_tools`
- `apply_patch` when `model_capabilities.supports_apply_patch_tool`

Everything else becomes deferrable under the existing deferred-tool policy, **except**:

- Planning: when `config.planning_active`, keep `start_planning`, `task_tracker`, and the read-only inspection set
  (`read_file`, `list_files`, `grep_file`, `code_search`) eager.
- `keeps_entry_available` (always_available_tools overrides) continues to win over deferral.
- `agent`, `list_skills`, `load_skill`, `load_skill_resource` defer like any other builtin; `search_tools` / catalog
  search can surface them.

Request payload shape, tool JSON schemas (except description trims in S2-budgets), and `ThreadEvent` contracts stay
unchanged.

### S2-docs

- `docs/development/EXECUTION_POLICY.md`: update the first-request budget numbers and the harness-tax subsection; note
  Minimal is the default.
- `docs/config/CONFIG_FIELD_REFERENCE.md` reflects the new defaults.
- Startup warning copy aligned with S2-prompt.

## [S3] Out of Scope

- Changing Minimal/Default prompt _prose_ beyond what budget trimming requires.
- New config profiles or a `/lean` slash command.
- Removing any tool from the registry (deferral only).
- Provider cache-affinity or eval-metric changes (`harness-tax-observability` already landed).
- Raising or re-tuning budgets after this change.

## Tasks

- [x] T1: SystemPromptMode::Minimal as enum default + fallback sites — acceptance:
      `SystemPromptMode::default() == Minimal`; missing-config paths use the enum default; parse still accepts all four
      values; Default profile text unchanged (covers: S2-prompt)
- [x] T2: budget tests at 1800 / 6k / 8k with description trim — acceptance:
      `emitted_model_tool_schema_fits_within_first_request_budget` passes at ≤1800;
      `first_request_total_token_budget_within_limit` passes at ≤6k/≤8k using the effective default prompt; description
      contract tests still pass (covers: S2-budgets; depends: T1)
- [x] T3: Codex-4 always-eager + planning exception — acceptance: unit tests show
      `task_tracker`/`start_planning`/`agent`/`list_skills`/`load_skill`/`load_skill_resource` are deferred outside
      planning; planning_active keeps planner + read-only inspection eager;
      `default_profile_exposes_only_codex_baseline_tools` still holds (covers: S2-tools)
- [x] T4: docs + startup warning copy — acceptance: EXECUTION_POLICY budget numbers and CONFIG_FIELD_REFERENCE default
      match the implementation; startup warning no longer claims lightweight is default (covers: S2-docs; depends: T1,
      T2, T3)
