# VTCode Project Analysis

## Executive Summary
- **Architecture**: Binary entry point in `src/` orchestrates commands and delegates to reusable logic in `vtcode-core`, which houses configuration, LLM provider abstractions, tooling, and TUI rendering. 【F:src/main.rs†L5-L145】【F:vtcode-core/src/llm/providers/openrouter.rs†L1-L41】【F:vtcode-core/src/config/models.rs†L1-L119】
- **Strengths**: Clear separation between CLI and core library, comprehensive provider support, and centralized configuration constants promote consistency. 【F:Cargo.toml†L1-L103】【F:vtcode-core/src/config/models.rs†L1-L207】
- **Risks**: Several monolithic modules exceed 1.5k lines, increasing cognitive load and slowing iteration (e.g., TUI session management, unified turn loop, ACP integration). 【982f89†L1-L10】【F:vtcode-core/src/ui/tui/session.rs†L1-L160】【F:src/agent/runloop/unified/turn.rs†L1-L200】【F:src/acp/zed.rs†L1-L200】
- **Opportunities**: Introduce layered module boundaries, extract reusable services, and codify naming/documentation standards to improve maintainability and onboarding speed.

## Architecture Overview
### Workspace Structure
- Workspace is a two-crate Rust project: CLI binary (`vtcode`) depends on the `vtcode-core` library; workspace members declared in root `Cargo.toml`. 【F:Cargo.toml†L1-L61】
- Entry point parses CLI arguments, loads configuration, and dispatches subcommands to domain-specific handlers. 【F:src/main.rs†L5-L145】

### Core Library (`vtcode-core`)
- **Configuration system**: Centralized provider metadata and model identifiers defined in `config/models.rs`, guarding against hardcoded strings and exposing provider capabilities. 【F:vtcode-core/src/config/models.rs†L1-L207】
- **LLM providers**: Concrete integrations (e.g., OpenRouter) implement streaming, tool-call handling, and reasoning trace reconciliation atop a shared provider trait. 【F:vtcode-core/src/llm/providers/openrouter.rs†L1-L120】
- **Agent runtime**: `core/agent/runner.rs` coordinates tool registries, MCP clients, and thread events for specialized agent runs. 【F:vtcode-core/src/core/agent/runner.rs†L1-L170】
- **Terminal UI**: `ui/tui/session.rs` implements Ratatui-based rendering, modal handling, and inline command UI, but concentrates significant UI state in a single file. 【F:vtcode-core/src/ui/tui/session.rs†L1-L160】

### CLI Binary (`src/`)
- **Runloop**: `agent/runloop/unified/turn.rs` manages conversation phases, model selection, tool invocation, and UI orchestration per turn. 【F:src/agent/runloop/unified/turn.rs†L1-L200】
- **ACP Integration**: `acp/zed.rs` bridges Zed’s Agent Client Protocol with VTCode’s tool registry and permission flows. 【F:src/acp/zed.rs†L1-L200】

## Codebase Health Snapshot
- Repository contains ~95k lines of Rust across first-party and vendored crates; single largest file is generated MCP types (6.7k LOC). Large application modules include the TUI session (4.3k LOC), unified turn loop (3.3k LOC), and ACP integration (2.6k LOC). 【982f89†L1-L10】
- Vendored MCP schema (`third-party/mcp-types`) is regenerated code; while large, it is mechanically derived and rarely edited, but still adds compile-time weight. 【F:third-party/mcp-types/src/v2024_11_05/types.rs†L1-L120】
- Core runtime modules couple many responsibilities (UI rendering, tool orchestration, configuration I/O), making them harder to test and reuse. 【F:src/agent/runloop/unified/turn.rs†L1-L200】【F:vtcode-core/src/ui/tui/session.rs†L1-L160】

## Maintainability Hotspots
| Area | Symptoms | Impact |
| --- | --- | --- |
| `vtcode-core/src/ui/tui/session.rs` | 4.3k LOC covering rendering, input handling, modal state, fuzzy search, and slash suggestions in one module. 【F:vtcode-core/src/ui/tui/session.rs†L1-L160】 | Steep learning curve; difficult to unit-test or switch UI backends.
| `src/agent/runloop/unified/turn.rs` | Manages context curation, tool routing, theme selection, archive navigation, and MCP events in a single turn loop. 【F:src/agent/runloop/unified/turn.rs†L1-L200】 | Hard to reason about turn lifecycles; increases risk of regressions when adding features.
| `src/acp/zed.rs` | Bundles protocol negotiation, permission prompts, workspace trust sync, and tool registry plumbing. 【F:src/acp/zed.rs†L1-L200】 | Limits reuse for other ACP clients; challenging to stub for integration tests.
| `vtcode-core/src/llm/providers/openrouter.rs` | Contains streaming assembly, tool-call normalization, and reasoning trace stitching. 【F:vtcode-core/src/llm/providers/openrouter.rs†L1-L120】 | Difficult to isolate provider-specific bugs; lacks clear boundary between HTTP client, adapters, and stream parser.
| `vtcode-core/src/config/models.rs` | Macro-generated OpenRouter catalog spans hundreds of entries within same file. 【F:vtcode-core/src/config/models.rs†L19-L119】 | Editing model lists is error-prone; code review friction from large diffs.

## Targeted Improvements
1. **Modularize UI session**
   - Extract modal management, list filtering, and slash command search into dedicated modules (`ui/tui/modal`, `ui/tui/search`).【F:vtcode-core/src/ui/tui/session.rs†L1-L160】
   - Introduce trait-based renderer interfaces to decouple Ratatui specifics for testing.
2. **Decompose unified turn loop**
   - Split turn execution into state machine (`TurnState`), UI interaction layer, and tool routing pipeline to isolate responsibilities.【F:src/agent/runloop/unified/turn.rs†L1-L200】
   - Move configuration lookups and provider verification into reusable services inside `vtcode-core`.
3. **Refine ACP adapter**
   - Separate Zed-specific protocol translation from generic ACP tool plumbing, enabling alternate clients or mock drivers.【F:src/acp/zed.rs†L1-L200】
   - Encapsulate permission dialog formatting and workspace trust synchronization for reuse across transports.
4. **Provider abstraction cleanup**
   - Introduce shared helper modules for streaming delta assembly and tool-call reconstruction to reduce duplication across providers.【F:vtcode-core/src/llm/providers/openrouter.rs†L1-L120】
5. **Model catalog management**
   - Generate OpenRouter listings from structured metadata (`docs/models.json`) at build time or split into smaller files grouped by vendor for maintainability.【F:vtcode-core/src/config/models.rs†L19-L119】

## Refactoring Plan
1. **Foundational Layering (Weeks 1-2)**
   - Define cross-cutting interfaces (`TurnDriver`, `UiSession`, `AcpClientAdapter`) inside `vtcode-core` with clear responsibilities.【F:vtcode-core/src/core/agent/runner.rs†L1-L170】【F:src/agent/runloop/unified/turn.rs†L1-L200】
   - Add unit tests for extracted utilities (e.g., fuzzy search, modal state transitions) before moving logic.
2. **UI Decomposition (Weeks 2-4)**
   - Move modal/list state structs into `ui/tui/modal.rs`, retaining session orchestrator as thin coordinator.【F:vtcode-core/src/ui/tui/session.rs†L37-L140】
   - Implement command palette module with dedicated tests to verify filtering and highlighting.
3. **Turn Loop Service Extraction (Weeks 3-5)**
   - Create `agent/runloop/context_manager.rs` for context pruning and summarization; isolate UI-specific output pipelines.【F:src/agent/runloop/unified/turn.rs†L54-L75】
   - Introduce asynchronous pipelines for tool execution results to decouple from UI thread.
4. **ACP Adapter Rework (Weeks 4-6)**
   - Extract configuration gating, permission flows, and workspace trust logic into reusable components shared with CLI-based tools.【F:src/acp/zed.rs†L40-L121】
   - Prepare integration tests that simulate ACP messages using lightweight mock channels.
5. **Provider & Config Cleanup (Weeks 5-7)**
   - Factor streaming utilities into `llm/providers/shared/streaming.rs` to reduce repeated code in provider implementations.【F:vtcode-core/src/llm/providers/openrouter.rs†L1-L120】
   - Split model catalog macros into vendor-specific files or move to generated constants to simplify diffs.【F:vtcode-core/src/config/models.rs†L19-L119】

## Code Quality & Best Practice Strategies
- **Consistent naming**: Enforce snake_case for functions/variables and PascalCase for types through Clippy and CI; document conventions in CONTRIBUTING. 【F:AGENTS.md†L96-L128】
- **Documentation**: Ensure public APIs and newly extracted modules include Rustdoc comments and update relevant docs under `./docs/`. 【F:AGENTS.md†L165-L193】
- **Separation of concerns**: Keep configuration parsing, command dispatch, and UI rendering in distinct crates/modules to preserve testability. 【F:Cargo.toml†L1-L103】【F:src/main.rs†L5-L145】
- **Testing strategy**: Expand unit and integration tests around newly factored components; leverage existing `tests/` harness and encourage scenario-based testing for turn loop and ACP flows.
- **Performance monitoring**: Profile large modules after refactors (e.g., `cargo nextest`, criterion benchmarks) to ensure decompositions maintain throughput.
- **Vendored code management**: Automate MCP schema updates and document regeneration steps to keep third-party code isolated from manual edits. 【F:third-party/mcp-types/src/v2024_11_05/types.rs†L1-L120】

## Next Steps
- Prioritize creation of module-level design docs for refactored components.
- Establish coding guidelines for contributions (naming, formatting, module structure) and link them from README/CONTRIBUTING.
- Introduce CI checks for large file thresholds to catch future monolithic growth early.
