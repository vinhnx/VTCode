# VT Code Refactor Proposal

## Overview
This document outlines a phased refactor strategy for VT Code to improve maintainability, configurability, and performance while preserving existing behavior. The plan is based on a review of the current codebase with emphasis on the CLI entrypoint, LLM orchestration, context management, and tool execution subsystems.

## Assessment Highlights
- The CLI `main.rs` mixes argument parsing, workspace resolution, configuration loading, theme handling, and agent bootstrap logic inside a single function, making the startup path difficult to reason about and hard to test in isolation.【F:src/main.rs†L5-L200】
- The orchestration router couples heuristics, provider inference, and async LLM routing inside one module, duplicating provider lookup logic already present in the LLM factory and complicating future extensions.【F:vtcode-core/src/core/router.rs†L5-L200】
- The context curator owns token accounting, decision ledger access, and phase detection while relying on rough string-length heuristics to estimate token usage, which limits accuracy and reusability across the codebase.【F:vtcode-core/src/core/context_curator.rs†L1-L200】
- The tool registry aggregates workspace state, tool instances, policy enforcement, MCP integration, PTY management, and planning into a single struct, leading to wide interfaces and cross-cutting responsibilities that hinder targeted testing.【F:vtcode-core/src/tools/registry/mod.rs†L1-L200】

## Refactor Goals
1. **Isolate startup concerns** so CLI execution paths are composable, testable, and reusable by future frontends.
2. **Centralize provider/model routing logic** to reduce duplication and simplify the introduction of new providers or routing strategies.
3. **Improve context management accuracy and extensibility** by decoupling token estimation and phase classification from stateful context assembly.
4. **Modularize the tool system** to clarify boundaries between registration, policy enforcement, execution, and external (MCP) integrations.

## Proposed Phased Plan

### Phase 1 – Entrypoint Modularization (High Priority)
- Extract workspace resolution, configuration loading, and theme setup into dedicated helper modules or builder structs invoked from `main`, returning a single `StartupContext` object that downstream commands can consume.【F:src/main.rs†L43-L200】
- Introduce unit tests around the extracted helpers (e.g., workspace validation, theme selection fallbacks) using synthetic directories to validate error messaging without invoking the full Tokio runtime.
- Update CLI command handlers to accept the structured startup context instead of recomputing provider/model/theme information, reducing reliance on shared mutable state.

### Phase 2 – Routing & Provider Cohesion (High Priority)
- Move model→provider inference currently repeated in the router into reusable helpers on the LLM factory or `ModelId`, and inject those helpers into router logic to avoid ad-hoc string parsing.【F:vtcode-core/src/core/router.rs†L135-L199】
- Split heuristic classification from model selection: expose a pure `TaskClassifier` and a separate `ModelSelector` that consumes configuration plus classifier output, enabling easier experimentation with ML-based routers or telemetry-driven tuning.【F:vtcode-core/src/core/router.rs†L40-L118】
- Add targeted tests for the classifier and selector to validate thresholds (e.g., patch detection, retrieval keywords) and make thresholds configurable via `vtcode.toml` instead of hardcoding.

### Phase 3 – Context Management Refinement (Medium Priority)
- Abstract token estimation into a shared service (backed by `tiktoken`-based implementations) instead of repeated `len()/4` heuristics, enabling consistent budgeting across context curator, summarizer, and cache layers.【F:vtcode-core/src/core/context_curator.rs†L96-L126】
- Separate phase detection and ledger summarization into strategy traits so alternative heuristics or telemetry-informed strategies can be swapped without modifying core curator logic.【F:vtcode-core/src/core/context_curator.rs†L171-L199】
- Add integration tests that simulate multi-phase conversations to ensure curated context respects `max_tokens_per_turn` while retaining the most relevant artifacts.

### Phase 4 – Tool Registry Decomposition (Medium Priority)
- Introduce a `ToolInventory` responsible only for constructing and storing tool instances, while delegating policy checks to a dedicated `ToolPolicyGateway` and PTY/session tracking to a `PtySessionManager`. This will shrink the `ToolRegistry` struct surface area.【F:vtcode-core/src/tools/registry/mod.rs†L44-L200】
- Replace `HashMap<&'static str, usize>` indexing with stronger typed identifiers or enums to prevent mismatched registrations and improve discoverability for MCP tools.【F:vtcode-core/src/tools/registry/mod.rs†L115-L200】
- Provide focused tests for policy enforcement, MCP discovery, and PTY quota handling by mocking the respective components instead of instantiating the entire registry.

### Phase 5 – Continuous Hardening (Low Priority)
- Consolidate duplicated provider registration code in the LLM factory by introducing a macro or trait-driven registration helper, ensuring all providers consistently honor prompt caching and base URL overrides.【F:vtcode-core/src/llm/factory.rs†L1-L200】
- Audit configuration modules to ensure defaults and validation live in one place (e.g., loader vs. defaults module) and document expected precedence rules within `docs/` for contributors.
- Establish profiling benchmarks in `benches/` to measure improvements after each phase, particularly focusing on startup latency, routing decisions, and tool execution overhead.

## Expected Outcomes
- Clearer separation of concerns that reduces cognitive load for new contributors and simplifies future feature work.
- Improved test coverage across startup, routing, context, and tool subsystems, catching regressions earlier.
- More accurate token management and routing decisions that directly benefit runtime performance and agent reliability.
- A modular foundation that enables new UI surfaces, providers, or tools to plug in with minimal changes to existing code.

## Next Steps
1. Socialize this plan with maintainers and confirm prioritization.
2. Create tracking issues per phase with granular subtasks.
3. Begin Phase 1 extraction work behind feature flags or incremental PRs to keep changes reviewable.
