# Agent Runner Refactor Plan

## Current State

-   `vtcode-core/src/core/agent/runner.rs` spans ~2.3k lines with multiple concerns intertwined (event recording, tool orchestration, streaming logic, conversation preparation).
-   Structs such as `ExecEventRecorder`, `ActiveCommand`, and `StreamingAgentMessage` live inline, making reuse difficult and increasing cognitive load.
-   Turn execution loop handles provider branching, streaming aggregation, tool invocation, and bookkeeping in a single monolithic function.
-   Error handling mixes logging, event emission, and control flow, reducing clarity.

## Refactor Goals

1. Improve readability and maintainability by enforcing single-responsibility functions and moving reusable components into dedicated modules.
2. Reduce code duplication across provider-specific branches while preserving existing behavior and telemetry.
3. Strengthen error handling to use `anyhow::Context` consistently and centralize warning emission.
4. Preserve backward compatibility for public APIs while introducing targeted unit/integration tests around refactored components.

## Proposed Work Breakdown

1. **Module Extraction**
    - Move `ExecEventRecorder` and related helper structs into `core/agent/events.rs` (or similar) with a clean public interface.
    - Expose narrow methods for turn lifecycle + streaming updates to simplify the runner.
2. **Conversation Preparation Cleanup**
    - Extract conversation/message building into helper functions that operate on lightweight data structures.
    - Introduce dedicated types for system instruction and task context assembly.
3. **Streaming & Response Handling**
    - Isolate streaming aggregation into a helper that returns a structured result (`ProviderResponseSummary`), capturing final text, reasoning, and warnings.
    - Remove duplicated logging by routing through standardized event hooks.
4. **Tool Invocation & Loop Control**
    - Encapsulate the turn execution loop into a manager struct with clear exit conditions and instrumentation.
    - Simplify provider branching by abstracting provider-specific capabilities into strategy traits where feasible.
5. **Testing & Validation**
    - Add focused unit tests for the extracted event recorder module.
    - Introduce integration tests covering streaming fallbacks and tool-loop limits using mocked providers.
    - Run `cargo fmt`, `cargo clippy`, `cargo check`, and targeted `cargo nextest` suites.

## Open Questions & Risks

-   Need to ensure MCP initialization and tool registry wiring remain intact when splitting modules.
-   Must verify no regressions in structured logging/event sink integrations used by downstream consumers.
-   Consider incremental rollout to avoid destabilizing ongoing feature work on the same file.

## Next Actions

1. Confirm scope with maintainers (vinhnx) and align on module boundaries.
2. Start with module extraction (Step 1) while keeping behavior identical.
3. Follow up with conversation and streaming refactors once foundational pieces are separated.

## Task Tracker

-   [x] Author technical design note for `core/agent/events.rs` public API. → see `docs/project/agent_runner_events_design.md`
-   [x] Extract `ExecEventRecorder` + helper structs into new module with unit tests.
-   [x] Replace inline conversation builders with helper functions (`conversation.rs`).
-   [x] Introduce streaming response helper returning `ProviderResponseSummary` struct.
-   [x] Refactor turn execution loop to use helper modules and reduce branching.
-   [x] Update or add tests covering event recorder, streaming fallbacks, and loop guard.
-   [ ] Run full QA pass (`cargo fmt`, `cargo clippy --workspace --all-targets`, `cargo check`, targeted `cargo nextest`). _(blocked: `cargo nextest run -p vtcode-core` fails with pre-existing async regressions in bootstrap.rs, lib.rs, project_doc.rs, and mcp tests)_
-   [ ] Draft changelog entry summarizing refactor scope.

## Dependencies & Coordination

-   Confirm upcoming feature work on agent runner to avoid merge conflicts; coordinate with contributors active on `codex/enhance-ast-grep-code-grep-functionality`.
-   Determine availability of mocked LLM provider utilities for integration testing; if missing, schedule follow-up task to build lightweight mocks.
-   Ensure documentation updates (developer guide, prompts) remain synchronized with new helper modules.

## Success Criteria

-   Monolithic runner file reduced significantly in size with clear module boundaries.
-   Test coverage maintained or improved, especially around event emission and streaming fallbacks.
-   No regressions reported in automated tool policies or MCP integration during validation.
-   Code passes clippy and adheres to project error-handling conventions (`anyhow::Context`).
