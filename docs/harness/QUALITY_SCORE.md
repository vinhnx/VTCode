# Quality Score

Quality grading for each VT Code domain. Grades are A (excellent), B (good), C (needs improvement), D (critical attention required).

Dimensions: **Test Coverage**, **API Stability**, **Agent Legibility**, **Error Handling**, **Documentation**

Last reviewed: 2026-02-16

---

## Scoring Method

| Grade | Criteria                                                                                                     |
| ----- | ------------------------------------------------------------------------------------------------------------ |
| A     | Strong automated coverage, stable interfaces, low ambiguity for agents, actionable errors, and current docs. |
| B     | Solid baseline with manageable gaps; no critical weaknesses but clear improvement areas remain.              |
| C     | Material quality gaps or staleness that slow delivery and increase risk.                                     |
| D     | Critical weaknesses that threaten reliability, safety, or maintainability.                                   |

### Evidence Policy

- Score updates require current repository evidence (tests, docs, tracker status, or code references).
- Priority actions must be verifiable by command or explicit artifact check.
- If a prior priority action is resolved, replace it with the next highest-impact gap.

---

## LLM System

**Scope**: `vtcode-llm/`, `vtcode-core/src/llm/`
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes                                                                                              |
| ---------------- | ----- | ------------------------------------------------------------------------------------------------------------- |
| Test Coverage    | B     | Provider factory and request shaping have unit tests. Failover paths still need broader integration coverage. |
| API Stability    | B     | Multi-provider factory pattern is stable. Model metadata remains centralized in `docs/models.json`.           |
| Agent Legibility | B     | Clear provider abstraction and extension pattern.                                                             |
| Error Handling   | A     | Uses `anyhow::Result` with context; provider error handling includes failover pathways.                       |
| Documentation    | B     | `docs/providers/PROVIDER_GUIDES.md` exists and is maintained.                                                           |

**Overall: B**
**Priority action**: add integration tests that exercise multi-provider failover behavior under provider failure.
**Verify**: `cargo nextest run --test integration_tests`

---

## Tool System

**Scope**: `vtcode-tools/`, `vtcode-core/src/tools/`
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes                                                                          |
| ---------------- | ----- | ----------------------------------------------------------------------------------------- |
| Test Coverage    | B     | Core tool traits and unified tools are covered; some handlers still lack dedicated tests. |
| API Stability    | A     | `Tool`, `ModeTool`, and `CacheableTool` traits are stable and composable.                 |
| Agent Legibility | A     | Registry and trait patterns are predictable for extension.                                |
| Error Handling   | A     | Unified error model with severity and retryability is in place.                           |
| Documentation    | B     | System-level docs exist; per-tool docs are uneven.                                        |

**Overall: A-**
**Priority action**: add tests for high-use handlers without dedicated coverage.
**Verify**: `cargo nextest run`

---

## Configuration

**Scope**: `vtcode-config/`
**Related debt**: TD-010 (resolved)

| Dimension        | Grade | Evidence / Notes                                                                         |
| ---------------- | ----- | ---------------------------------------------------------------------------------------- |
| Test Coverage    | B     | Config loading and schema validation are covered; precedence edge cases can be expanded. |
| API Stability    | B     | Precedence chain (env -> toml -> constants) is well-defined with schema-backed config.   |
| Agent Legibility | B     | Precedence and schema docs make extension paths discoverable.                            |
| Error Handling   | B     | Load-time validation is present; some messages can still be more task-oriented.          |
| Documentation    | A-    | `docs/config/CONFIG_FIELD_REFERENCE.md` now provides generated field-level reference.    |

**Overall: B+**
**Priority action**: extend tests for precedence edge cases and malformed overrides.
**Verify**: `cargo nextest run -p vtcode-config`

---

## Security

**Scope**: `vtcode-process-hardening/`, sandbox system, command safety
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes                                                                                                                                                                        |
| ---------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Test Coverage    | B     | Command safety and file-ops boundary regressions are covered; process-hardening env filtering tests now include additional edge cases. OS-specific hardening still has CI depth limits. |
| API Stability    | B     | Security boundary model and tool policy behavior are stable.                                                                                                                            |
| Agent Legibility | B     | Security docs are organized (`docs/development/PROCESS_HARDENING.md`, `docs/security/`).                                                                                                            |
| Error Handling   | A     | Hardening paths expose explicit failure codes and policy outcomes.                                                                                                                      |
| Documentation    | A     | Security docs are consolidated with index and quick reference.                                                                                                                          |

**Overall: B+**
**Priority action**: expand sandbox/policy integration tests across more allow/deny edge matrices and OS-specific hardening scenarios.
**Verify**: `cargo nextest run -p vtcode-process-hardening`

---

## MCP Integration

**Scope**: `vtcode-core/src/mcp/`
**Related debt**: TD-008 (in-progress)

| Dimension        | Grade | Evidence / Notes                                                                                                                                                                                                  |
| ---------------- | ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Test Coverage    | B     | Transport/discovery coverage is in place, with lifecycle reinit/idempotent shutdown plus startup-failure and partial-provider-failure paths now covered. Full OAuth callback flow remains pending implementation. |
| API Stability    | B     | `McpClient`, `McpProvider`, and `McpToolExecutor` interfaces are stable; HTTP transport remains experimental.                                                                                                     |
| Agent Legibility | A     | `docs/mcp/MCP_INTEGRATION_GUIDE.md` and module layout are clear and comprehensive.                                                                                                                                |
| Error Handling   | B     | Timeout and concurrency controls are present; diagnostics can be tightened on some error paths.                                                                                                                   |
| Documentation    | A     | Dedicated guide plus `docs/mcp/00_START_HERE.md` provide strong integration guidance.                                                                                                                             |

**Overall: B+**
**Priority action**: add full OAuth callback-flow integration tests once callback implementation lands.
**Verify**: `cargo nextest run --test integration_tests`

---

## Subagent System

**Scope**: `vtcode-core/src/subagents/`, `vtcode-config/src/subagent.rs`
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes                                                                                                                                       |
| ---------------- | ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Test Coverage    | B     | Built-in types plus custom loading paths are now tested in `vtcode-core` and `vtcode-config`; isolation/failure propagation coverage is still limited. |
| API Stability    | B     | `spawn_subagent` contract and definition format are stable.                                                                                            |
| Agent Legibility | B     | `docs/subagents/SUBAGENTS.md` documents creation and usage paths.                                                                                      |
| Error Handling   | B     | Isolation reduces blast radius; error reporting format can be more structured.                                                                         |
| Documentation    | B     | Core guide exists; complex interaction patterns are only partially documented.                                                                         |

**Overall: B**
**Priority action**: add isolation and failure-propagation tests for subagent runner execution paths.
**Verify**: `cargo nextest run -p vtcode-core`

---

## PTY/Exec

**Scope**: `vtcode-bash-runner/`, `vtcode-core/src/exec/`
**Related debt**: TD-007 (in-progress)

| Dimension        | Grade | Evidence / Notes                                                                                                                                                            |
| ---------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Test Coverage    | B+    | PTY command/session paths now include timeout and output-truncation regressions in `vtcode-core/tests/pty_tests.rs`, alongside existing session lifecycle and runner tests. |
| API Stability    | B     | Standard/PTY/streaming execution modes are stable.                                                                                                                          |
| Agent Legibility | B     | `unified_exec` usage is clear; lower-level PTY plumbing is harder to navigate.                                                                                              |
| Error Handling   | B     | Exit code and timeout handling are in place; shell-init edge paths can improve.                                                                                             |
| Documentation    | B     | Runner docs and workflow docs exist and are current.                                                                                                                        |

**Overall: B**
**Priority action**: expand PTY regression coverage for additional shell-init and cross-platform behavior paths.
**Verify**: `cargo test -p vtcode-core --test pty_tests && cargo test -p vtcode-bash-runner --test pipe_tests`

---

## TUI

**Scope**: `src/` (Ratatui interface)
**Related debt**: TD-005 (resolved)

| Dimension        | Grade | Evidence / Notes                                                                                                                                                                                            |
| ---------------- | ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Test Coverage    | B+    | Inline event-loop mapping now has focused regressions for key actions (`LaunchEditor`, `ToggleMode`, team switch, plan confirmation, interrupt exit) in `src/agent/runloop/unified/inline_events/tests.rs`. |
| API Stability    | B     | Event loop and keybinding behavior are stable.                                                                                                                                                              |
| Agent Legibility | B     | Large-handler debt was addressed; navigation and structure are more legible than prior review.                                                                                                              |
| Error Handling   | B     | Terminal restoration and cleanup behavior are robust.                                                                                                                                                       |
| Documentation    | B     | TUI startup and testing guidance are available.                                                                                                                                                             |

**Overall: B**
**Priority action**: add higher-level integration tests that exercise full inline loop interactions with modal flows and queue editing.
**Verify**: `cargo test -p vtcode --bin vtcode inline_events::tests`

---

## Optimized Code Understanding & Bash Safety

**Scope**: `vtcode-indexer/`, `vtcode-core/src/command_safety/`
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes                                                                                                                                                            |
| ---------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Test Coverage    | B+    | Critical bash command parsing has high test coverage. LLM-native understanding is empirically validated via evals.                                                         |
| API Stability    | A     | Shift to LLM-native understanding simplified the system and removed heavy grammar dependencies.                                                                             |
| Agent Legibility | A     | Unified tool guidance clearly explains the shift to LLM-native semantic analysis and the importance of bash safety.                                                         |
| Error Handling   | B+    | Robust fallbacks for unparseable shell commands; LLM handles syntax errors in general programming languages.                                                                |
| Documentation    | B+    | `docs/protocols/LANGUAGE_SUPPORT.md` and `docs/user-guide/tree-sitter-integration.md` updated to reflect the new architecture.                                                        |

**Overall: B+**
**Priority action**: expand bash safety tests with more complex obfuscation patterns and edge-case shell syntax.
**Verify**: `cargo nextest run -p vtcode-core --test shell_parser_tests`

---

## Documentation

**Scope**: `docs/` (654 files at review time on 2026-02-16)
**Related debt**: TD-001 (in-progress)

| Dimension        | Grade | Evidence / Notes                                                                                                                                                    |
| ---------------- | ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Test Coverage    | B     | Entry-point docs now have automated link-integrity checks via `scripts/check_docs_links.py` in CI.                                                                  |
| API Stability    | B     | Top-level docs boundary is now CI-enforced via `scripts/check_markdown_location.py` and `scripts/docs_top_level_allowlist.txt`, reducing uncontrolled sprawl drift. |
| Agent Legibility | B     | `docs/INDEX.md` now routes by active domains and explicit archive paths instead of a single historical initiative focus.                                            |
| Error Handling   | B     | Docs governance checks now emit remediation-oriented failures for broken links and placement violations (`check_docs_links.py`, `check_markdown_location.py`).      |
| Documentation    | B     | Core entrypoint docs (`AGENTS.md`, `docs/INDEX.md`, harness index) are aligned; consolidation remains active but now bounded by CI guardrails.                      |

**Overall: B**
**Priority action**: reduce existing top-level allowlist by moving high-churn historical docs from `docs/*.md` into domain folders or archive paths.
**Verify**: `python3 scripts/check_markdown_location.py && python3 scripts/check_docs_links.py && find docs -maxdepth 1 -type f -name "*.md" | wc -l`

---

## Summary Table

| Domain          | Overall | Priority Action                                                            | Status             |
| --------------- | ------- | -------------------------------------------------------------------------- | ------------------ |
| Tool System     | A-      | Add handler-level tests for remaining gaps.                                | maintenance        |
| LLM System      | B       | Add failover integration coverage.                                         | active improvement |
| Configuration   | B+      | Expand precedence/override edge-case tests.                                | active improvement |
| Security        | B+      | Add sandbox boundary integration tests.                                    | active improvement |
| MCP Integration | B+      | Complete OAuth and lifecycle negative-path tests.                          | active improvement |
| Subagent System | B       | Add custom-loading and isolation tests.                                    | active improvement |
| PTY/Exec        | B       | Expand PTY regression coverage for shell-init and cross-platform behavior. | active improvement |
| Tree-Sitter / Safety | B+      | Expand bash safety tests with obfuscation patterns.                       | maintenance        |
| TUI                 | B       | Add higher-level integration tests for modal flows and queue editing.      | active improvement |
| Documentation       | B       | Burn down top-level docs allowlist through ongoing consolidation/archival. | active improvement |
