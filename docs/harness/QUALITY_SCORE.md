# Quality Score

Quality grading for each VT Code domain. Grades are A (excellent), B (good), C (needs improvement), D (critical attention required).

## Snapshot

| Field                        | Value                                                 |
| ---------------------------- | ----------------------------------------------------- |
| Base domain review           | 2026-02-16                                            |
| Latest targeted revalidation | 2026-05-16 (TUI hotspot refresh + legibility tooling) |
| Latest rubric refresh        | 2026-05-16 (Agent Legibility guidance)                |

## Dimensions

| Dimension        | What it measures                                                                                       |
| ---------------- | ------------------------------------------------------------------------------------------------------ |
| Test Coverage    | Critical-path automated coverage and regression depth.                                                 |
| API Stability    | Whether interfaces and extension seams change predictably.                                             |
| Agent Legibility | Whether an agent can find the right entrypoints, docs, and active blockers without exploratory thrash. |
| Error Handling   | Whether failures are explicit, contextual, and remediation-oriented.                                   |
| Documentation    | Whether docs are current, cross-linked, and aligned with code.                                         |

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

### Agent Legibility Rubric

See [AGENT_LEGIBILITY_GUIDE.md](AGENT_LEGIBILITY_GUIDE.md) and [ARCHITECTURAL_INVARIANTS.md](ARCHITECTURAL_INVARIANTS.md) for the underlying harness rules.

Score `Agent Legibility` against these four checks before choosing a grade.

| Check                 | Review question                                                                                              |
| --------------------- | ------------------------------------------------------------------------------------------------------------ |
| Entrypoint discovery  | Can an agent find the right start file, guide, or module within 1-2 hops from the obvious repo entrypoints?  |
| Change-path locality  | Can a common change be completed without spelunking through oversized files or scattered control flow?       |
| Constraint visibility | Are debt, invariants, experimental paths, and caveats visible near the entrypoint instead of buried in code? |
| Recovery guidance     | If an agent lands on the wrong surface, do docs, errors, or cross-references redirect it quickly?            |

| Grade | Threshold          | Required signals                                                                                                                                                                       |
| ----- | ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A     | All 4 checks pass  | Primary entrypoints, extension seams, active debt, and caveats are easy to locate from docs or module layout. Common work stays on bounded paths instead of large-file archaeology.    |
| B     | 3 of 4 checks pass | Main paths are discoverable, but at least one common flow still requires extra module hops, a large hotspot, or code-first reconstruction. The gap is explicit and tracked.            |
| C     | 2 of 4 checks pass | Agents must reconstruct the system by hopping through code, stale docs, or ambiguous ownership boundaries. Important constraints are incomplete or only discoverable after deep reads. |
| D     | 0-1 checks pass    | Navigation is ambiguous enough that agents are likely to modify the wrong surface, miss critical constraints, or repeat exploratory loops without converging.                          |

Use these downgrade rules even if the prose note sounds optimistic.

| Condition                                                                                                              | Highest allowed grade |
| ---------------------------------------------------------------------------------------------------------------------- | --------------------- |
| A common task path still depends on one or more active oversized hotspot modules or high-friction orchestration roots. | B                     |
| Active debt, experimental paths, or ownership boundaries are missing from the nearest entrypoint docs.                 | C                     |
| Docs, tracker state, and code disagree about where the work should happen.                                             | C                     |
| A reviewer cannot point to a concrete proving artifact for the score.                                                  | B                     |

### Agent Legibility Evidence Pattern

When updating an `Agent Legibility` row, make all three signals explicit in the note.

| Include in note                 | Why                                       |
| ------------------------------- | ----------------------------------------- |
| What is easy to find            | Makes the positive signal concrete.       |
| What still causes search thrash | Keeps the grade falsifiable.              |
| Which artifact proves it        | Lets a reviewer verify the claim quickly. |

---

## LLM System

**Scope**: `vtcode-llm/`, `vtcode-core/src/llm/`
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes                                                                                                                                       |
| ---------------- | ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Test Coverage    | B     | Provider factory and request shaping have unit tests. Failover paths still need broader integration coverage.                                          |
| API Stability    | B     | Multi-provider factory pattern is stable. Model metadata remains centralized in `docs/models.json`.                                                    |
| Agent Legibility | B     | Provider factory and provider guides make the main extension seam obvious; end-to-end failover behavior still requires reading tests and runtime flow. |
| Error Handling   | A     | Uses `anyhow::Result` with context; provider error handling includes failover pathways.                                                                |
| Documentation    | B     | `docs/providers/PROVIDER_GUIDES.md` exists and is maintained.                                                                                          |

**Overall: B**
**Priority action**: add integration tests that exercise multi-provider failover behavior under provider failure.
**Verify**: `cargo nextest run --test integration_tests`

---

## Tool System

**Scope**: `vtcode-tools/`, `vtcode-core/src/tools/`
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes                                                                                                                 |
| ---------------- | ----- | -------------------------------------------------------------------------------------------------------------------------------- |
| Test Coverage    | B     | Core tool traits and unified tools are covered; some handlers still lack dedicated tests.                                        |
| API Stability    | A     | `Tool`, `ModeTool`, and `CacheableTool` traits are stable and composable.                                                        |
| Agent Legibility | A     | Registry, trait seams, and handler composition are predictable to extend; per-tool discovery still depends on uneven local docs. |
| Error Handling   | A     | Unified error model with severity and retryability is in place.                                                                  |
| Documentation    | B     | System-level docs exist; per-tool docs are uneven.                                                                               |

**Overall: A-**
**Priority action**: add tests for high-use handlers without dedicated coverage.
**Verify**: `cargo nextest run`

---

## Configuration

**Scope**: `vtcode-config/`
**Related debt**: TD-010 (resolved)

| Dimension        | Grade | Evidence / Notes                                                                                                                                          |
| ---------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Test Coverage    | B     | Config loading and schema validation are covered; precedence edge cases can be expanded.                                                                  |
| API Stability    | B     | Precedence chain (env -> toml -> constants) is well-defined with schema-backed config.                                                                    |
| Agent Legibility | B     | The env -> toml -> constants path and generated field reference make overrides easy to trace; malformed-override behavior still needs code or test reads. |
| Error Handling   | B     | Load-time validation is present; some messages can still be more task-oriented.                                                                           |
| Documentation    | A-    | `docs/config/CONFIG_FIELD_REFERENCE.md` now provides generated field-level reference.                                                                     |

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
| Agent Legibility | B     | Process-hardening and security docs make policy boundaries discoverable; some allow/deny edge matrices still require code-level tracing.                                                |
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
| Agent Legibility | A     | The start-here guide, integration guide, and module layout make provider lifecycle paths easy to locate; experimental HTTP transport remains a clearly caveated side path.                                        |
| Error Handling   | B     | Timeout and concurrency controls are present; diagnostics can be tightened on some error paths.                                                                                                                   |
| Documentation    | A     | Dedicated guide plus `docs/mcp/00_START_HERE.md` provide strong integration guidance.                                                                                                                             |

**Overall: B+**
**Priority action**: add full OAuth callback-flow integration tests once callback implementation lands.
**Verify**: `cargo nextest run --test integration_tests`

---

## PTY/Exec

**Scope**: `vtcode-bash-runner/`, `vtcode-core/src/exec/`
**Related debt**: TD-007 (in-progress)

| Dimension        | Grade | Evidence / Notes                                                                                                                                                            |
| ---------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Test Coverage    | B+    | PTY command/session paths now include timeout and output-truncation regressions in `vtcode-core/tests/pty_tests.rs`, alongside existing session lifecycle and runner tests. |
| API Stability    | B     | Standard/PTY/streaming execution modes are stable.                                                                                                                          |
| Agent Legibility | B     | `unified_exec` is the stable top-level entrypoint; PTY lifecycle and shell-init flows still force deeper traversal through lower-level plumbing.                            |
| Error Handling   | B     | Exit code and timeout handling are in place; shell-init edge paths can improve.                                                                                             |
| Documentation    | B     | Runner docs and workflow docs exist and are current.                                                                                                                        |

**Overall: B**
**Priority action**: expand PTY regression coverage for additional shell-init and cross-platform behavior paths.
**Verify**: `cargo test -p vtcode-core --test pty_tests && cargo test -p vtcode-bash-runner --test pipe_tests`

---

## TUI

**Scope**: `src/` (Ratatui interface)
**Related debt**: TD-005 (in-progress)

| Dimension        | Grade | Evidence / Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ---------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Test Coverage    | B+    | Inline event-loop mapping still has focused regressions for key actions (`LaunchEditor`, primary-agent switching, planning confirmation, interrupt exit) in `src/agent/runloop/unified/inline_events/tests.rs`, and helper behavior now has focused coverage in both `src/agent/runloop/unified/turn/session/interaction_loop_runner/support.rs` and `src/agent/runloop/unified/turn/session_loop_runner/support.rs`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| API Stability    | B     | Event loop and keybinding behavior are stable.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| Agent Legibility | B     | The tracked TUI roots now advertise entrypoints, constraints, and verify commands in-code: `unified/session_setup/ui.rs` is 464 lines after extracting `ui/local_agents.rs`, `ui/persistent_memory.rs`, `ui/tests.rs`, `ui/resume_render.rs`, and `ui/header_context.rs`; `turn/context.rs` is 276 lines after extracting `context/continuation.rs`, `context/message_history.rs`, `context/tests.rs`, `context/runtime_context.rs`, and `context/response_handling.rs`; `turn/tool_outcomes/execution_result.rs` is 441 lines after extracting tool-output probe, failure-path, and test helpers; `slash_commands/diagnostics/memory.rs` is 499 lines after extracting config-persistence, prompt, presentation, and navigation helpers; and the planning turn-processing root is 334 lines after extracting interview helpers. `scripts/check_agent_legibility.py` still reports zero missing headers and zero delegation gaps across the tracked roots, so the tracked TD-005 hotspot set stays fully below 500 lines. After the latest compaction pass, `turn/compaction/mod.rs` is down to 840 lines after extracting `compaction/file_read_dedup.rs`, `compaction/memory_envelope.rs`, and `compaction/recovery_preview.rs`, with `compaction/memory_envelope/local_summary.rs` and `compaction/memory_envelope/persistence.rs` pulling the memory-envelope path into smaller units; common TUI work can still spill into remaining oversized roots led by `turn/turn_processing/llm_request/copilot_runtime.rs` (1921), `turn/compaction/tests.rs` (1847), `turn/session/slash_commands/agents_authoring.rs` (1773), `turn/session_loop_runner/mod.rs` (1299), `turn/session/slash_commands/oauth.rs` (1294), and `turn/session/slash_commands/agents/runtime.rs` (1210). |
| Error Handling   | B     | Terminal restoration and cleanup behavior are robust.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Documentation    | B     | TUI startup and testing guidance are available.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |

**Overall: B**
**Priority action**: finish the compaction surface by splitting `turn/compaction/tests.rs` and pushing `turn/compaction/mod.rs` below the 500-line invariant, then continue the next TD-005 passes on `turn/turn_processing/llm_request/copilot_runtime.rs`, `turn/session/slash_commands/agents_authoring.rs`, and `turn/session_loop_runner/mod.rs`, while adding higher-level integration tests that exercise full inline loop interactions with modal flows and queue editing.
**Verify**: `find src -name '*.rs' -type f -exec wc -l {} + | sort -nr | head -n 20 && find src/agent/runloop/unified/turn/compaction -name '*.rs' -type f -exec wc -l {} + | sort -nr && python3 scripts/check_agent_legibility.py --mode warn && cargo test -p vtcode --bin vtcode inline_events::tests`

---

## Optimized Code Understanding & Bash Safety

**Scope**: `vtcode-indexer/`, `vtcode-core/src/command_safety/`
**Related debt**: none open

| Dimension        | Grade | Evidence / Notes                                                                                                               |
| ---------------- | ----- | ------------------------------------------------------------------------------------------------------------------------------ |
| Test Coverage    | B+    | Critical bash command parsing has high test coverage. LLM-native understanding is empirically validated via evals.             |
| API Stability    | A     | Shift to LLM-native understanding simplified the system and removed heavy grammar dependencies.                                |
| Agent Legibility | A     | Docs clearly separate LLM-native code understanding from retained bash-parser safety paths; the boundary is easy to follow.    |
| Error Handling   | B+    | Robust fallbacks for unparseable shell commands; LLM handles syntax errors in general programming languages.                   |
| Documentation    | B+    | `docs/protocols/LANGUAGE_SUPPORT.md` and `docs/user-guide/tree-sitter-integration.md` updated to reflect the new architecture. |

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
| Agent Legibility | B     | `docs/INDEX.md` and harness entrypoints expose active domains and archive routes; overall doc breadth still raises search cost outside curated entrypoints.         |
| Error Handling   | B     | Docs governance checks now emit remediation-oriented failures for broken links and placement violations (`check_docs_links.py`, `check_markdown_location.py`).      |
| Documentation    | B     | Core entrypoint docs (`AGENTS.md`, `docs/INDEX.md`, harness index) are aligned; consolidation remains active but now bounded by CI guardrails.                      |

**Overall: B**
**Priority action**: reduce existing top-level allowlist by moving high-churn historical docs from `docs/*.md` into domain folders or archive paths.
**Verify**: `python3 scripts/check_markdown_location.py && python3 scripts/check_docs_links.py && find docs -maxdepth 1 -type f -name "*.md" | wc -l`

---

## Summary Table

| Domain               | Overall | Priority Action                                                                                                                                   | Status             |
| -------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------ |
| Tool System          | A-      | Add handler-level tests for remaining gaps.                                                                                                       | maintenance        |
| LLM System           | B       | Add failover integration coverage.                                                                                                                | active improvement |
| Configuration        | B+      | Expand precedence/override edge-case tests.                                                                                                       | active improvement |
| Security             | B+      | Add sandbox boundary integration tests.                                                                                                           | active improvement |
| MCP Integration      | B+      | Complete OAuth and lifecycle negative-path tests.                                                                                                 | active improvement |
| PTY/Exec             | B       | Expand PTY regression coverage for shell-init and cross-platform behavior.                                                                        | active improvement |
| Tree-Sitter / Safety | B+      | Expand bash safety tests with obfuscation patterns.                                                                                               | maintenance        |
| TUI                  | B       | Decompose the remaining 1500+ line TUI orchestration/support modules, keep hotspot legibility checks clean, and add modal-flow integration tests. | active improvement |
| Documentation        | B       | Burn down top-level docs allowlist through ongoing consolidation/archival.                                                                        | active improvement |
