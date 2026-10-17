---
feature: harness-tax-observability
status: delivered
updated: 2026-09-27
branch: feat/harness-tax-apply
commits: ace5ba394..58030951d
---

# Harness Tax Observability

## Report

**What was built** — VT Code now measures and surfaces the per-call harness tax that HarnessTax (Pan et al.) shows can
dominate coding-agent cost. Every assembled LLM request logs `token_budget_breakdown` with `first_call` and
`fixed_overhead_tokens` (system instructions + tool schemas). The session's first successful request build is captured
once into `SessionStats::first_call_composition` and shown on exit as `First-call overhead N (system S + tools T)`.
Independently, `vtcode-eval` reports HarnessTax-style cost-efficiency metrics per suite: `cost_per_solve` (total known
cost / successful attempts; `None` if any attempt is unpriced or nothing passed), `mean_cost_per_attempt`,
`mean_tokens_per_attempt`, and `mean_turns_per_attempt`, rendered as an `Efficiency:` markdown line.

No request payload, tool schema, or prompt text changed. The default Codex-baseline tool surface, Progressive schema
budgets, and cache-affinity work were already in place; this change makes their tax measurable.

**Verification** — `./scripts/check-dev.sh` PASS. `cargo nextest run --no-fail-fast` **8603 passed, 4 failed, 12
skipped**; the 4 failures are **PRE-EXISTING** (confirmed identical on stashed clean base `ace5ba394`):
`cli_harness_failures::print_mode_requires_prompt_or_stdin` (auth env, documented in gotchas),
`compaction::tests::refresh_session_memory_envelope_merges_existing_continuity_fields`,
`tool_outcomes::handlers::tests::runtime::registry_exhaustion_latches_runloop_and_blocks_the_next_inspection`,
`vtcode-core tools::registry::tests::harness_terminal_runs_retain_completed_sessions_until_close`. New unit tests all
PASS (`first_call_composition_captures_once`, `first_call_composition_captures_on_first_build_only`,
`stats_line_includes_first_call_overhead_when_present`, `stats_line_omits_zero_first_call_overhead`, four
`cost_efficiency`/`cost_per_solve`/`token_and_turn` cases, `markdown_renders_efficiency_line`; full `vtcode-eval`
43/43). Independent review verdict: **approve**.

### Journey log

- HarnessTax's "first call" is the first _assembled_ request, not `step_count == 1` (step resets every user turn).
  Capture-once on `SessionStats` is the correct latch.
- `reset_for_fresh_execution` deliberately keeps `first_call_composition` with aggregate usage/cost — it is session-run
  diagnostics, not conversational context.
- Review medium findings (dual priced-cost predicate + results clone; missing two-build `first_call` test) were fixed in
  `58030951d` before delivery.
- `.vtcode/memory/` is not shared with git worktrees; the HarnessTax library entry lives in the main checkout's
  `library.md`.
- Unpriced attempts must never count as free in `cost_per_solve`; the metric is `None` whenever the cost basis is
  incomplete.

## [S1] Problem

HarnessTax (Pan et al., <https://harnesstax.github.io/>) shows the same model can cost up to 5x more under a different
coding-agent harness at essentially the same success rate. Most of that gap is **first-call fixed overhead**
(instructions + tool schemas), not more turns. VT Code already budgets this (Progressive schemas ≤ ~2.2k tokens, first
request ≤ 12k) and logs `token_budget_breakdown` per turn, but:

1. Users cannot see the first-call fixed overhead ("harness tax") without reading trajectory logs. The exit summary
   reports session totals only.
2. Eval reports expose `cost_usd` totals and pass@k, but not **cost per solve**, **tokens per attempt**, or **turns per
   attempt** — the figures the paper uses to compare harnesses. VT Code therefore cannot be placed on a cost-success
   frontier the way the study places Pi / Codex / Claude Code.

## [S2] Design

Settled scope (grill 2026-09-27): observability + eval metrics; means and cost_per_solve only (no bootstrap CIs).

### S2-first-call: first-call harness-tax composition

- Reuse the existing per-turn `token_budget_breakdown` measurement in
  `src/agent/runloop/unified/turn/turn_processing/llm_request/` (system prompt tokens, tool-schema tokens,
  message-history tokens).
- Add two derived fields to the breakdown record and log line:
  - `first_call: bool` — true when this is the session's first assembled LLM request (capture-once latch on
    `SessionStats`; not `step_count == 1`, which resets every user turn).
  - `fixed_overhead_tokens: usize` — `system_prompt_tokens + tool_schema_tokens`. This is the harness tax per call; on
    the first call it is the paper's initial harness context (excluding the task prompt).
- Capture the first-call composition once into the interactive `SessionStats` (`src/agent/runloop/unified/state.rs`) as
  `first_call_composition: Option<FirstCallComposition>` with `system_prompt_tokens`, `tool_schema_tokens`,
  `message_history_tokens`, `on_wire_tools`. `fixed_overhead_tokens()` is derived.
- Surface it once on session exit in `postamble::build_stats_line` / `ExitData` as a stats fragment when present, e.g.
  `First-call overhead 3.2k (system 1.1k + tools 2.1k)`. Zero-valued totals omit the fragment. No new slash command.
- Do not change request payload shape, tool schemas, or prompt text. This is measurement + display only.

### S2-eval: cost-efficiency metrics in eval reports

In `crates/codegen/vtcode-eval`:

- Aggregate per suite while already summing `cost_usd` and merging `trace_summary`:
  - `mean_cost_per_attempt: Option<f64>` — `all_cost_usd / known_cost_runs` when `known_cost_runs > 0`.
  - `cost_per_solve: Option<f64>` — `all_cost_usd / passed_runs` when `passed_runs > 0` **and** every run that
    contributed cost is accounted for (`unpriced_runs == 0`). Unpriced runs keep the metric `None` rather than treating
    them as free (matches the existing "unknown cost is not zero" rule).
  - `mean_tokens_per_attempt: Option<f64>` — mean of `(input_tokens + output_tokens)` over attempts that carry a
    `trace_summary`; `None` when no trace summaries exist.
  - `mean_turns_per_attempt: Option<f64>` — mean of `trace_summary.turns` over attempts that carry one.
- `passed_runs` is attempt-level (count of `EvalRunResult` with `RunOutcome::Pass`), matching the paper's resolve-rate
  denominator (`total cost / successful attempts`).
- Markdown report line under the aggregate: `Efficiency: cost/solve $X.XXXX · $Y per attempt · Z tokens · W turns` when
  the values exist.
- Serialize the new fields on `SuiteReport`. Do not change `EvalMetric` / pass@k / pass^k shapes.
- Share one `priced_cost` predicate between the executor summation and `CostEfficiency::from_runs`.

### S2-docs: research mapping

- Record the HarnessTax claims and the VT Code counterpart in `.vtcode/memory/library.md` (main checkout; gitignored,
  not shared with worktrees).
- Add a short "Harness tax" subsection to `docs/development/EXECUTION_POLICY.md` (Auditing token cost area) naming the
  first-call breakdown fields and the eval metrics, so the feature stays discoverable.

## [S3] Out of Scope

- Bootstrap confidence intervals on cost/success (deferred).
- A SWE-bench / Terminal-Bench runner or cross-harness comparison harness.
- Changing the default tool surface, Progressive schema budgets, or SystemPromptMode defaults.
- New config profiles or `/harness-tax` slash command.
- Provider cache-affinity work (already delivered in `harness-stability-cost-p1` / `provider-cache-affinity-p2`).

## Tasks

- [x] T1: first-call composition fields on token_budget_breakdown + SessionStats capture — acceptance: unit/serial tests
      show `first_call` true only on the session's first request build, `fixed_overhead_tokens == system + tools`, and
      the captured `FirstCallComposition` is recorded once (covers: S2-first-call)
- [x] T2: exit-postamble first-call overhead fragment — acceptance: `build_stats_line` includes `First-call overhead …`
      when composition is present and omits it when absent/zero; existing stats-line tests still pass (covers:
      S2-first-call; depends: T1)
- [x] T3: eval suite cost-efficiency metrics — acceptance: tests cover cost_per_solve (all priced + at least one pass),
      None when unpriced runs exist, mean tokens/turns from trace summaries, None when no traces; markdown report
      renders the new line (covers: S2-eval)
- [x] T4: docs + library entry — acceptance: EXECUTION_POLICY harness-tax subsection and library.md entry cite
      HarnessTax and name the new fields (covers: S2-docs; depends: T1, T3)
