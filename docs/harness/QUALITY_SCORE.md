# Quality Score

Quality grading for each VTCode domain. Grades are A (excellent), B (good), C (needs improvement), D (critical attention required).

Dimensions: **Test Coverage**, **API Stability**, **Agent Legibility**, **Error Handling**, **Documentation**

Last reviewed: 2026-02-16

---

## Scoring Method

| Grade | Criteria |
|-------|----------|
| A     | Strong automated coverage, stable interfaces, low ambiguity for agents, actionable errors, and current docs. |
| B     | Solid baseline with manageable gaps; no critical weaknesses but clear improvement areas remain. |
| C     | Material quality gaps or staleness that slow delivery and increase risk. |
| D     | Critical weaknesses that threaten reliability, safety, or maintainability. |

### Evidence Policy

- Score updates require current repository evidence (tests, docs, tracker status, or code references).
- Priority actions must be verifiable by command or explicit artifact check.
- If a prior priority action is resolved, replace it with the next highest-impact gap.

---

## LLM System

**Scope**: `vtcode-llm/`, `vtcode-core/src/llm/`
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes |
|------------------|-------|------------------|
| Test Coverage    | B     | Provider factory and request shaping have unit tests. Failover paths still need broader integration coverage. |
| API Stability    | B     | Multi-provider factory pattern is stable. Model metadata remains centralized in `docs/models.json`. |
| Agent Legibility | B     | Clear provider abstraction and extension pattern. |
| Error Handling   | A     | Uses `anyhow::Result` with context; provider error handling includes failover pathways. |
| Documentation    | B     | `docs/PROVIDER_GUIDES.md` exists and is maintained. |

**Overall: B**
**Priority action**: add integration tests that exercise multi-provider failover behavior under provider failure.
**Verify**: `cargo nextest run --test integration_tests`

---

## Tool System

**Scope**: `vtcode-tools/`, `vtcode-core/src/tools/`
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes |
|------------------|-------|------------------|
| Test Coverage    | B     | Core tool traits and unified tools are covered; some handlers still lack dedicated tests. |
| API Stability    | A     | `Tool`, `ModeTool`, and `CacheableTool` traits are stable and composable. |
| Agent Legibility | A     | Registry and trait patterns are predictable for extension. |
| Error Handling   | A     | Unified error model with severity and retryability is in place. |
| Documentation    | B     | System-level docs exist; per-tool docs are uneven. |

**Overall: A-**
**Priority action**: add tests for high-use handlers without dedicated coverage.
**Verify**: `cargo nextest run`

---

## Configuration

**Scope**: `vtcode-config/`
**Related debt**: TD-010 (resolved)

| Dimension        | Grade | Evidence / Notes |
|------------------|-------|------------------|
| Test Coverage    | B     | Config loading and schema validation are covered; precedence edge cases can be expanded. |
| API Stability    | B     | Precedence chain (env -> toml -> constants) is well-defined with schema-backed config. |
| Agent Legibility | B     | Precedence and schema docs make extension paths discoverable. |
| Error Handling   | B     | Load-time validation is present; some messages can still be more task-oriented. |
| Documentation    | A-    | `docs/config/CONFIG_FIELD_REFERENCE.md` now provides generated field-level reference. |

**Overall: B+**
**Priority action**: extend tests for precedence edge cases and malformed overrides.
**Verify**: `cargo nextest run -p vtcode-config`

---

## Security

**Scope**: `vtcode-process-hardening/`, sandbox system, command safety
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes |
|------------------|-------|------------------|
| Test Coverage    | C     | Command safety is tested; sandbox and OS-specific hardening paths remain thin in CI. |
| API Stability    | B     | Security boundary model and tool policy behavior are stable. |
| Agent Legibility | B     | Security docs are organized (`docs/PROCESS_HARDENING.md`, `docs/security/`). |
| Error Handling   | A     | Hardening paths expose explicit failure codes and policy outcomes. |
| Documentation    | A     | Security docs are consolidated with index and quick reference. |

**Overall: B+**
**Priority action**: add integration tests for sandbox boundary enforcement and policy outcomes.
**Verify**: `cargo nextest run -p vtcode-process-hardening`

---

## MCP Integration

**Scope**: `vtcode-core/src/mcp/`
**Related debt**: TD-008 (in-progress)

| Dimension        | Grade | Evidence / Notes |
|------------------|-------|------------------|
| Test Coverage    | C     | Basic transport/discovery coverage exists; OAuth callback and lifecycle edge cases are incomplete. |
| API Stability    | B     | `McpClient`, `McpProvider`, and `McpToolExecutor` interfaces are stable; HTTP transport remains experimental. |
| Agent Legibility | A     | `docs/mcp/MCP_INTEGRATION_GUIDE.md` and module layout are clear and comprehensive. |
| Error Handling   | B     | Timeout and concurrency controls are present; diagnostics can be tightened on some error paths. |
| Documentation    | A     | Dedicated guide plus `docs/mcp/00_START_HERE.md` provide strong integration guidance. |

**Overall: B+**
**Priority action**: complete OAuth callback-flow and lifecycle negative-path integration coverage.
**Verify**: `cargo nextest run --test integration_tests`

---

## Subagent System

**Scope**: `vtcode-core/src/subagents/`, `vtcode-config/src/subagent.rs`
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes |
|------------------|-------|------------------|
| Test Coverage    | B     | Built-in types plus custom loading paths are now tested in `vtcode-core` and `vtcode-config`; isolation/failure propagation coverage is still limited. |
| API Stability    | B     | `spawn_subagent` contract and definition format are stable. |
| Agent Legibility | B     | `docs/subagents/SUBAGENTS.md` documents creation and usage paths. |
| Error Handling   | B     | Isolation reduces blast radius; error reporting format can be more structured. |
| Documentation    | B     | Core guide exists; complex interaction patterns are only partially documented. |

**Overall: B**
**Priority action**: add isolation and failure-propagation tests for subagent runner execution paths.
**Verify**: `cargo nextest run -p vtcode-core`

---

## PTY/Exec

**Scope**: `vtcode-bash-runner/`, `vtcode-core/src/exec/`
**Related debt**: TD-007 (in-progress)

| Dimension        | Grade | Evidence / Notes |
|------------------|-------|------------------|
| Test Coverage    | B-    | PTY behavior is inherently hard to test; tmux workflow provides a path but not full automation yet. |
| API Stability    | B     | Standard/PTY/streaming execution modes are stable. |
| Agent Legibility | B     | `unified_exec` usage is clear; lower-level PTY plumbing is harder to navigate. |
| Error Handling   | B     | Exit code and timeout handling are in place; shell-init edge paths can improve. |
| Documentation    | B     | Runner docs and workflow docs exist and are current. |

**Overall: B**
**Priority action**: promote tmux-based PTY checks into repeatable automated integration coverage.
**Verify**: `cargo nextest run`

---

## TUI

**Scope**: `src/` (Ratatui interface)
**Related debt**: TD-005 (resolved)

| Dimension        | Grade | Evidence / Notes |
|------------------|-------|------------------|
| Test Coverage    | B-    | Unit testing remains hard; tmux workflow improves integration-test feasibility. |
| API Stability    | B     | Event loop and keybinding behavior are stable. |
| Agent Legibility | B     | Large-handler debt was addressed; navigation and structure are more legible than prior review. |
| Error Handling   | B     | Terminal restoration and cleanup behavior are robust. |
| Documentation    | B     | TUI startup and testing guidance are available. |

**Overall: B**
**Priority action**: add focused regression tests around event-loop behavior and keybinding handling.
**Verify**: `cargo nextest run --test integration_tests`

---

## Tree-Sitter / Code Intelligence

**Scope**: `vtcode-indexer/`, `vtcode-core/src/tree_sitter/`
**Related debt**: TD-009 (resolved)

| Dimension        | Grade | Evidence / Notes |
|------------------|-------|------------------|
| Test Coverage    | B     | Language parsing and code-intelligence operations have test coverage. |
| API Stability    | B     | Operation APIs and caching approach are stable. |
| Agent Legibility | B     | Operation dispatch and language extension patterns are understandable. |
| Error Handling   | B     | Parser unavailability fallbacks are handled gracefully. |
| Documentation    | B     | `docs/LANGUAGE_SUPPORT.md` now documents the language support matrix; `docs/vtcode_indexer.md` remains concise. |

**Overall: B**
**Priority action**: expand indexer docs with additional operational examples and troubleshooting paths.
**Verify**: `rg --line-number \"^#|^##\" docs/vtcode_indexer.md docs/LANGUAGE_SUPPORT.md`

---

## Documentation

**Scope**: `docs/` (666 files at review time on 2026-02-16)
**Related debt**: TD-001 (in-progress)

| Dimension        | Grade | Evidence / Notes |
|------------------|-------|------------------|
| Test Coverage    | N/A   | Documentation is not tested via automated correctness checks. |
| API Stability    | C     | Many one-off implementation docs still risk staleness drift. |
| Agent Legibility | C     | Discoverability remains hard at current volume despite better entry points. |
| Error Handling   | N/A   | Not applicable. |
| Documentation    | C     | Core maps improved (`AGENTS.md`, harness index), but consolidation is incomplete. |

**Overall: C**
**Priority action**: continue consolidation and archival for stale one-off docs, prioritized by high-traffic domains.
**Verify**: `find docs -type f | wc -l`

---

## Summary Table

| Domain            | Overall | Priority Action | Status |
|-------------------|---------|-----------------|--------|
| Tool System       | A-      | Add handler-level tests for remaining gaps. | maintenance |
| LLM System        | B       | Add failover integration coverage. | active improvement |
| Configuration     | B+      | Expand precedence/override edge-case tests. | active improvement |
| Security          | B+      | Add sandbox boundary integration tests. | active improvement |
| MCP Integration   | B+      | Complete OAuth and lifecycle negative-path tests. | active improvement |
| Subagent System   | B       | Add custom-loading and isolation tests. | active improvement |
| PTY/Exec          | B       | Convert tmux workflow into repeatable automation. | active improvement |
| Tree-Sitter       | B       | Deepen operational docs beyond support matrix. | maintenance |
| TUI               | B       | Add regression tests for event loop/keybindings. | active improvement |
| Documentation     | C       | Continue consolidation/archival to reduce sprawl. | active improvement |
