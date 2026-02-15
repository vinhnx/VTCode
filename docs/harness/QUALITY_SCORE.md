# Quality Score

Quality grading for each VTCode domain. Grades are A (excellent), B (good), C (needs improvement), D (critical attention required).

Dimensions: **Test Coverage**, **API Stability**, **Agent Legibility**, **Error Handling**, **Documentation**

Last reviewed: 2026-02-15

---

## LLM System

**Scope**: `vtcode-llm/`, `vtcode-core/src/llm/`

| Dimension        | Grade | Notes |
|------------------|-------|-------|
| Test Coverage    | B     | Provider factory and request shaping have unit tests. Failover paths need more integration tests. |
| API Stability    | B     | Multi-provider factory pattern is stable. Model metadata in `docs/models.json` is maintained. |
| Agent Legibility | B     | Clear provider abstraction. Agents can add providers by following existing patterns. |
| Error Handling   | A     | Uses `anyhow::Result` with context throughout. Automatic failover on provider errors. (see `vtcode-llm/src/lib.rs`) |
| Documentation    | B     | Provider guides exist (`docs/PROVIDER_GUIDES.md`). Internal architecture could use more inline docs. |

**Overall: B**

---

## Tool System

**Scope**: `vtcode-tools/`, `vtcode-core/src/tools/`

| Dimension        | Grade | Notes |
|------------------|-------|-------|
| Test Coverage    | B     | Core traits tested. Unified tools (`unified_exec`, `unified_file`, `unified_search`) have coverage. Some of the 54+ handlers lack dedicated tests. |
| API Stability    | A     | `Tool`, `ModeTool`, `CacheableTool` traits are stable and well-composed. (see `vtcode-tools/src/lib.rs`, `vtcode-core/src/tools/unified_error.rs`) |
| Agent Legibility | A     | Trait-driven composition makes it clear how to add new tools. Registry pattern is predictable. |
| Error Handling   | A     | `UnifiedToolError` with `ErrorSeverity` and `is_retryable()`. Circuit breaker per tool. |
| Documentation    | B     | Architecture doc covers tool system. Individual tool docs vary in quality. |

**Overall: A-**

---

## Configuration

**Scope**: `vtcode-config/`

| Dimension        | Grade | Notes |
|------------------|-------|-------|
| Test Coverage    | B     | Config loading and schema validation tested. Edge cases around precedence could use more coverage. |
| API Stability    | B     | Precedence chain (env → toml → constants) is well-defined. Schema uses `schemars`. |
| Agent Legibility | B     | `docs/config/CONFIGURATION_PRECEDENCE.md` exists. Agents can find where to add new config fields. |
| Error Handling   | B     | Validation at load time. Some config errors could provide more actionable messages. |
| Documentation    | B     | Precedence documented. Individual field documentation is partial. |

**Overall: B**

---

## Security

**Scope**: `vtcode-process-hardening/`, sandbox system, command safety

| Dimension        | Grade | Notes |
|------------------|-------|-------|
| Test Coverage    | C     | Process hardening is OS-specific, hard to test in CI. Command safety has tests but sandbox coverage is thin. |
| API Stability    | B     | Security boundaries are well-defined. Tool policies (allow/deny/prompt) are stable. |
| Agent Legibility | B     | `docs/PROCESS_HARDENING.md`, `docs/SECURITY.md` exist. Agents understand the safety model. |
| Error Handling   | A     | Specific exit codes for hardening failures (5, 6, 7). Command safety uses explicit deny/allow. (see `vtcode-process-hardening/src/lib.rs` exit codes) |
| Documentation    | A     | Consolidated security docs into `docs/security/`. AGENTS.md and README.md provide clear pointers. |

**Overall: B+**

---

## MCP Integration

**Scope**: `vtcode-core/src/mcp/` (9 modules)

| Dimension        | Grade | Notes |
|------------------|-------|-------|
| Test Coverage    | C     | Transport layer and tool discovery have basic tests. OAuth flow and provider lifecycle need more coverage. |
| API Stability    | B     | `McpClient`, `McpProvider`, `McpToolExecutor` interfaces are stable. HTTP transport marked experimental. |
| Agent Legibility | A     | `docs/MCP_INTEGRATION_GUIDE.md` is thorough. Module structure is clear (client, provider, transport, executor). |
| Error Handling   | B     | Timeout management and per-provider concurrency control. Some error paths could surface better diagnostics. |
| Documentation    | A     | Integration guide, improvement designs, and roadmap all exist and are maintained. |

**Overall: B+**

---

## Subagent System

**Scope**: `vtcode-core/src/subagents/`, `vtcode-config/src/subagent.rs`

| Dimension        | Grade | Notes |
|------------------|-------|-------|
| Test Coverage    | C     | Built-in agent types tested. Custom agent loading and isolation boundaries need more tests. |
| API Stability    | B     | `spawn_subagent` API is stable. Agent definition format (Markdown + YAML frontmatter) is documented. |
| Agent Legibility | B     | `docs/subagents/SUBAGENTS.md` exists. Agents can define custom subagents by following examples. |
| Error Handling   | B     | Context isolation prevents cascading failures. Error reporting could be more structured. |
| Documentation    | B     | Subagent guide exists. Interaction patterns between lead and subagents could be better documented. |

**Overall: B**

---

## PTY/Exec

**Scope**: `vtcode-bash-runner/`, `vtcode-core/src/exec/`

| Dimension        | Grade | Notes |
|------------------|-------|-------|
| Test Coverage    | B-    | PTY session management hard to test in CI. New tmux-based TUI testing workflow (`.agent/workflows/pi-tui-test.md`) provides path for automated tests. |
| API Stability    | B     | Three execution modes (standard, PTY, streaming) are stable. |
| Agent Legibility | B     | Agents understand how to use `unified_exec`. Internal PTY plumbing is less legible. |
| Error Handling   | B     | Exit code handling and timeout management. Some PTY edge cases (shell init failures) could be more robust. |
| Documentation    | B     | Updated bash runner docs. PTY stability fixes documented. |

**Overall: B**

---

## TUI

**Scope**: `src/` (Ratatui interface)

| Dimension        | Grade | Notes |
|------------------|-------|-------|
| Test Coverage    | B-    | Hard to unit-test. Tmux workflow enables TUI integration testing. |
| API Stability    | B     | Widget composition and event loop are stable. Keybinding system is well-defined. |
| Agent Legibility | B     | Improved event handler legibility. AGENTS.md clarifies navigation rules. |
| Error Handling   | B     | Graceful terminal state restoration on panic. Signal handling for cleanup. |
| Documentation    | B     | Quick start guide and TUI testing workflow exist. |

**Overall: B-**

---

## Tree-Sitter / Code Intelligence

**Scope**: `vtcode-indexer/`, `vtcode-core/src/tree_sitter/`

| Dimension        | Grade | Notes |
|------------------|-------|-------|
| Test Coverage    | B     | Language-specific parsing tests exist. Code intelligence operations (goto_definition, find_references) tested. |
| API Stability    | B     | Operations API is stable. Incremental AST building with caching is mature. |
| Agent Legibility | B     | `code_intelligence.rs` has clear operation dispatch. Adding new languages follows a pattern. |
| Error Handling   | B     | Graceful fallback when tree-sitter parsers are unavailable. |
| Documentation    | C     | `docs/vtcode_indexer.md` exists but is minimal. Language support matrix not documented. |

**Overall: B**

---

## Documentation

**Scope**: `docs/` (649 files as of last count)

| Dimension        | Grade | Notes |
|------------------|-------|-------|
| Test Coverage    | N/A   | Documentation itself is not tested. |
| API Stability    | C     | Many docs are one-off implementation summaries that become stale. No deprecation policy. |
| Agent Legibility | C     | Volume makes discovery hard. 649 files with inconsistent naming. No clear hierarchy. |
| Error Handling   | N/A   | Not applicable. |
| Documentation    | C     | Meta-documentation improved with AGENTS.md and CONTRIBUTING.md. Gardening cadence established. |

**Overall: C**

---

## Summary Table

| Domain            | Overall | Priority Action |
|-------------------|---------|-----------------|
| Tool System       | A-      | Maintain. Add tests for remaining handlers. |
| LLM System        | B       | Add failover integration tests. |
| Configuration     | B       | Improve field-level documentation. |
| Security          | B+     | Consolidated security docs. Add sandbox tests. |
| MCP Integration   | B+      | Increase test coverage for OAuth and lifecycle. |
| Subagent System   | B       | Test custom agent loading. Document interaction patterns. |
| PTY/Exec          | B       | Improved PTY test coverage and docs via tmux workflow. |
| Tree-Sitter       | B       | Document language support matrix. |
| TUI               | B-      | Break up large event handler modules. Improve docs. |
| Documentation     | C       | Consolidate stale docs. AGENTS.md provides better entry points. |
