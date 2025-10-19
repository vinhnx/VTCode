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

## Refactoring TODO Checklist
- [x] Establish interface layer in `vtcode-core` for turn driving, UI sessions, and ACP adapters, ensuring responsibilities are documented and enforced by module boundaries.【F:vtcode-core/src/core/agent/runner.rs†L1-L170】【F:src/agent/runloop/unified/turn.rs†L1-L200】
    - Introduced the `core::interfaces` module with `TurnDriver`, `UiSession`, and `AcpClientAdapter` traits so the binary depends on narrowly scoped contracts instead of concrete implementations.【F:vtcode-core/src/core/interfaces/mod.rs†L1-L11】【F:vtcode-core/src/core/interfaces/turn.rs†L1-L38】【F:vtcode-core/src/core/interfaces/ui.rs†L1-L35】【F:vtcode-core/src/core/interfaces/acp.rs†L1-L26】
    - Wrapped the unified runloop in a `UnifiedTurnDriver` and wired the CLI to invoke it through the trait while updating tool routing to consume the new `UiSession` abstraction for event handling.【F:src/agent/runloop/unified/driver.rs†L1-L23】【F:src/agent/runloop/mod.rs†L1-L74】【F:src/agent/runloop/unified/tool_routing.rs†L1-L168】【F:src/agent/runloop/unified/turn.rs†L240-L520】
    - Exposed a `ZedAcpAdapter` implementing the shared ACP contract so the CLI launches the bridge through a trait-managed entry point.【F:src/acp/zed.rs†L1-L60】【F:src/cli/acp.rs†L1-L60】【F:src/acp/mod.rs†L1-L5】
- [x] Capture regression tests for existing UI behaviors (fuzzy search, modal transitions) prior to moving code to new modules.【F:vtcode-core/src/ui/tui/session.rs†L1-L160】
- [x] Extract modal and list management into `ui/tui/modal.rs` and related helpers, leaving `session.rs` as a coordinator over composed components.【F:vtcode-core/src/ui/tui/session.rs†L37-L140】
- [x] Introduce a command palette/search module with isolated filtering/highlighting logic and dedicated unit tests.
    - Implemented the slash palette state machine with prefix-aware highlighting and wraparound navigation tests, letting the session delegate rendering and suggestion updates to the shared helper.
    - Centralized slash command prefix and range parsing inside the palette helper so the session reuses the shared utilities, backed by unit tests for cursor edge cases.
    - Extended the shared slash palette to drive paging and home/end navigation shortcuts, ensuring the session defers all keyboard handling to the helper with dedicated coverage.
    - Introduced shared fuzzy search utilities and richer slash command ranking so palette suggestions stay relevant for substring and keyword matches beyond simple prefixes.
- [x] Split unified turn loop into state management, UI interaction, and tool routing submodules with clear API boundaries.【F:src/agent/runloop/unified/state.rs†L1-L58】【F:src/agent/runloop/unified/ui_interaction.rs†L1-L223】【F:src/agent/runloop/unified/tool_routing.rs†L1-L142】【F:src/agent/runloop/unified/turn.rs†L1-L120】
    - Extracted `SessionStats`, `CtrlCState`, and related turn lifecycle helpers into `state.rs`, isolating shared session bookkeeping from the orchestration loop.【F:src/agent/runloop/unified/state.rs†L1-L58】
    - Moved presentation helpers (session status rendering, placeholder spinners) to `ui_interaction.rs`, reducing `turn.rs`'s direct coupling to Ratatui handle internals.【F:src/agent/runloop/unified/ui_interaction.rs†L1-L223】
    - Relocated human-in-the-loop tool approval logic into `tool_routing.rs`, encapsulating prompt flows and policy updates behind a focused interface.【F:src/agent/runloop/unified/tool_routing.rs†L1-L142】
    - Shifted streaming response rendering and reasoning state tracking to `ui_interaction.rs`, keeping turn orchestration focused on decision flow while the helper manages markdown output and cancellation-aware cleanup.【F:src/agent/runloop/unified/ui_interaction.rs†L200-L441】
- [x] Relocate context pruning and summarization workflows into an `agent/runloop/context_manager.rs` service and integrate asynchronous pipelines for tool execution results.【F:src/agent/runloop/unified/context_manager.rs†L1-L111】【F:src/agent/runloop/unified/tool_pipeline.rs†L1-L70】
    - Centralized conversation trimming, curated prompt assembly, and token budget resets inside a reusable `ContextManager`, replacing ad-hoc helpers in the turn loop.【F:src/agent/runloop/unified/context_manager.rs†L13-L108】【F:src/agent/runloop/unified/turn.rs†L210-L275】
    - Introduced a dedicated `tool_pipeline` module that executes tools under timeout, surfaces stdout/metadata, and drives the refactored turn loop's success/failure handling without inline timeout logic.【F:src/agent/runloop/unified/tool_pipeline.rs†L1-L70】【F:src/agent/runloop/unified/turn.rs†L1385-L1539】
    - Upgraded tool failure telemetry to emit structured `ToolExecutionError` payloads in the UI and conversation history, preserving metadata for ledger entries instead of plain error strings.【F:src/agent/runloop/unified/turn.rs†L1504-L1567】
- [x] Decompose `src/acp/zed.rs` into reusable configuration gating, permission handling, and workspace trust components with mockable interfaces for testing.【F:src/acp/tooling.rs†L1-L420】【F:src/acp/permissions.rs†L1-L136】【F:src/acp/workspace.rs†L1-L31】【F:src/acp/zed.rs†L1-L212】
    - Hoisted the ACP tool declarations and capability gating into `acp/tooling.rs`, exposing a `ToolRegistryProvider` contract so tests can substitute registry behaviour without touching the Zed adapter.【F:src/acp/tooling.rs†L1-L420】【F:src/acp/zed.rs†L330-L420】
    - Moved permission prompt orchestration to `acp/permissions.rs`, implementing an `AcpPermissionPrompter` that emits consistent telemetry and is injected into `ZedAgent` through an `Rc` handle.【F:src/acp/permissions.rs†L1-L136】【F:src/acp/zed.rs†L823-L908】
    - Wrapped the workspace trust handshake behind `DefaultWorkspaceTrustSynchronizer` so the adapter depends on an interface instead of the free function, easing future test doubles.【F:src/acp/workspace.rs†L1-L31】【F:src/acp/zed.rs†L271-L310】
    - Centralized tool execution reporting constants in `acp/reports.rs` and refactored the Zed adapter to consume the shared helpers while keeping tool responses intact.【F:src/acp/reports.rs†L1-L69】【F:src/acp/zed.rs†L920-L1305】
- [x] Add integration tests that replay ACP message flows using mocks to validate the refactored adapters.
    - Added `tests/acp_integration.rs` to simulate allow, deny, cancel, and transport-failure permission paths with a fake ACP client and registry so the `DefaultPermissionPrompter` produces the expected tool reports for each outcome.【F:tests/acp_integration.rs†L1-L191】
    - Captured canned Zed payloads in `tests/acp_fixtures.rs` (backed by JSON fixtures) so the integration harness deserializes real session IDs, tool calls, and argument shapes when replaying workflows.【F:tests/acp_fixtures.rs†L1-L17】【F:tests/fixtures/acp/permission_read_file.json†L1-L17】【F:tests/fixtures/acp/permission_list_files.json†L1-L17】
    - Exposed ACP helper modules only when running tests and re-exported the fixtures through `tests/mod.rs`, letting mocks compile without widening the runtime surface area.【F:src/acp/mod.rs†L1-L16】【F:src/lib.rs†L97-L103】【F:tests/mod.rs†L1-L8】
- [x] Share streaming assembly and tool-call reconstruction utilities across providers via a `llm/providers/shared` module.【F:vtcode-core/src/llm/providers/shared/mod.rs†L1-L269】
    - Promoted the `ToolCallBuilder`, SSE parsing helpers, and stream delta accumulator into `shared/mod.rs`, letting providers reuse the same aggregation logic and tests instead of maintaining bespoke builders.【F:vtcode-core/src/llm/providers/shared/mod.rs†L23-L208】
    - Added provider-agnostic `StreamAssemblyError` conversion and `StreamTelemetry` hooks so adapters emit consistent diagnostics while layering provider-specific logging when needed.【F:vtcode-core/src/llm/providers/shared/mod.rs†L7-L65】【F:vtcode-core/src/llm/providers/openrouter.rs†L1-L80】【F:vtcode-core/src/llm/providers/openai.rs†L1-L55】
    - Refactored the OpenRouter and OpenAI adapters to depend on the shared utilities, removing duplicated SSE parsing helpers and wiring telemetry into their streaming loops without affecting feature-gated providers.【F:vtcode-core/src/llm/providers/openrouter.rs†L70-L360】【F:vtcode-core/src/llm/providers/openrouter.rs†L1828-L1910】【F:vtcode-core/src/llm/providers/openai.rs†L1-L120】【F:vtcode-core/src/llm/providers/openai.rs†L1600-L1712】
- [x] Automate or modularize OpenRouter model catalog generation to reduce edit distance and improve reviewability of model updates.【F:vtcode-core/src/config/models.rs†L1-L120】
    - Swapped the hand-maintained macro for build-generated variant definitions, metadata, and vendor groupings sourced from `docs/models.json` so `ModelId` helpers share a single data pipeline.【F:vtcode-core/build.rs†L1-L235】【F:vtcode-core/src/config/models.rs†L200-L340】
    - Generated OpenRouter constants and aliases at build time, including vendor-scoped slices (`config::models::openrouter::vendor::<slug>::MODELS`) to simplify downstream lookups.【F:vtcode-core/src/config/constants.rs†L100-L200】
    - Documented the workflow in `docs/contributing-models.md`, covering required JSON fields, regeneration steps, and verification commands for future catalog updates.【F:docs/contributing-models.md†L1-L47】
    - Fixed the generated metadata module to qualify `ModelId` and `OpenRouterMetadata` through `super::`, allowing the nested include to compile without missing type errors.【F:vtcode-core/build.rs†L232-L339】
    - Restored runtime dependencies (`rmcp`, `mcp-types`, and `tokenizers`) to the main crate manifest and derived the OpenRouter tool availability list directly from JSON metadata so function-calling guards compile under the new generation pipeline.【F:vtcode-core/Cargo.toml†L28-L120】【F:vtcode-core/build.rs†L1-L320】
- [x] Harden Windows startups by enforcing process mitigations that disable dynamic code, extension points, and untrusted image loads before the CLI continues.【F:src/process_hardening.rs†L1-L120】
- [x] Introduce CI enforcement that blocks oversized tracked files from landing in the repository, keeping future refactors reviewable.【F:.github/workflows/ci.yml†L23-L32】【F:scripts/check_large_files.py†L1-L87】
    - Added a reusable `scripts/check_large_files.py` helper that scans `git ls-files` output and fails when assets exceed the 400 KB ceiling, with allowlist overrides for future exceptions.【F:scripts/check_large_files.py†L1-L87】
    - Wired the large file guard into the main CI workflow so pull requests must satisfy the size budget before other jobs run.【F:.github/workflows/ci.yml†L23-L32】

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
