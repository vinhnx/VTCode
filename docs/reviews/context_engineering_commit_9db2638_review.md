# Review: Context Engineering Implementation (commit 9db2638b5a62e663a36dea02e658c0dfa590b044)

## Summary
- The commit introduces new modules (`ContextCurator`, `TokenBudgetManager` updates) and configuration surfaces that aim to follow Anthropic's context engineering guidance by curating per-turn context and budgeting tokens dynamically.
- Documentation and configuration samples comprehensively describe a multi-phase rollout ("Phase 1/2") and claim full integration across the agent loop.
- Practical integration gaps and a few implementation issues mean the promised context-engineering workflow is not yet realized in the runtime.

## Alignment with Anthropic Guidance
- Strength: *Iterative context curation:* The new `ContextCurator` encapsulates an ordered set of context sources (recent conversation, active files, ledger, errors, tool inventory) consistent with the "curate on every turn" loop described in Anthropic's guide.【F:vtcode-core/src/core/context_curator.rs†L292-L333】
- Observation: *System prompt calibration:* The refreshed default system prompt trims some verbose instructions, but it remains relatively prescriptive and still borders on the "too specific" side of Anthropic's calibration spectrum; further iteration might keep the guardrails while reducing procedural rigidity.【F:vtcode-core/src/prompts/system.rs†L10-L74】

## Key Issues to Address
1. **`ContextCurator` is not wired into the agent runtime.** Outside of unit tests and documentation snippets there is no code that instantiates or calls the curator, so no turn ever benefits from the new selection logic.【1213b6†L1-L15】
2. **Token budget tracking never updates live usage.** The only call sites for `count_tokens_for_component`/`count_tokens` are within tests, leaving runtime stats at zero and preventing the budget thresholds from ever firing.【1d1aa0†L1-L8】
3. **Conversation phase state is not persisted.** `detect_phase` returns a phase but never stores it; when heuristics fail on later turns the curator falls back to `self.current_phase`, which stays `Unknown` (except when errors occur). This breaks the intended phase-aware tool selection.【F:vtcode-core/src/core/context_curator.rs†L220-L258】
4. **Emoji usage violates repository policy.** `generate_report` emits "Unicode warning symbol (U+26A0 U+FE0F)" markers even though the project guidelines explicitly forbid emoji output.【F:vtcode-core/src/core/token_budget.rs†L311-L314】

## Recommended Fixes

| Issue | Recommended Fix | Owner / Effort | Success Criteria |
| --- | --- | --- | --- |
| Missing runtime curator wiring | Instantiate `ContextCurator` during session initialization and inject it into the turn executor so every completion request calls `curate_context`. | Runtime team · Medium | Assistant traces show curated context entries and regression tests confirm curator participation. |
| Dormant token budget counters | Route every inbound/outbound message through `TokenBudgetManager::count_tokens_for_component` and persist the running totals in state shared with the curator. | Runtime team · Medium | Token dashboards reflect non-zero usage and alerts trigger when configured thresholds are exceeded. |
| Phase state not persisted | Assign `self.current_phase = phase` (and persist to the session store) prior to returning from `detect_phase`, and reload this state when a session resumes. | Context team · Low | Subsequent turns without heuristic matches reuse the last known phase and phase-aware tool selection logic executes. |
| Emoji in reports | Swap emoji markers for plain-text labels (e.g., `WARNING`) while keeping structured logging fields intact. | Telemetry team · Low | Budget reports render without Unicode symbol usage violations. |

## Action Plan
- Integrate the curator into the runloop (e.g., session setup or turn execution) so each assistant call receives curated context, and ensure the decision ledger & active file tracking feed it continuously.
- Thread the `TokenBudgetManager` through message ingestion so every system/user/assistant/tool payload updates counts before curator reads the remaining budget.
- Persist detected phases by assigning `self.current_phase = phase` before returning, providing a graceful fallback when heuristics yield no match.
- Replace emoji in budget reports with plain-text markers to comply with logging guidelines.

Addressing these gaps—and tracking the success criteria above—will bring the implementation closer to the iterative, feedback-driven context engineering workflow outlined in Anthropic's reference material.
