# Tech Debt Tracker

Known technical debt items with priority and ownership. Review this document on a regular cadence (suggested: bi-weekly).

## Priority Levels

- **P0**: Blocks new feature work or causes user-visible issues. Fix immediately.
- **P1**: Significant quality or maintainability impact. Fix within current cycle.
- **P2**: Moderate impact. Schedule for next cycle.
- **P3**: Minor impact. Track but tolerate. Fix opportunistically.

## Status Values

- **open**: identified, not yet started
- **in-progress**: actively being worked on (link to exec plan if applicable)
- **resolved**: fixed, awaiting verification or merged
- **wont-fix**: accepted as-is with documented rationale

---

## Debt Items

| ID     | Area           | Description | Priority | Status | Last Reviewed | Notes |
|--------|----------------|-------------|----------|--------|---------------|-------|
| TD-001 | Documentation  | Documentation sprawl: 600+ files in `docs/`, many stale or redundant. See `QUALITY_SCORE.md` Documentation domain. | P1 | in-progress | 2026-02-16 | Added CI guardrails for top-level `docs/*.md` (`scripts/check_markdown_location.py` + `scripts/docs_top_level_allowlist.txt`) and entrypoint link integrity (`scripts/check_docs_links.py`). Consolidation/archival remains in-progress; docs count is 654 as of 2026-02-16. |
| TD-002 | Codebase       | Duplicated logic across workspace crates. Common patterns reimplemented instead of shared via `vtcode-commons`. | P2 | resolved | 2026-02-15 | Ref: `TD-002-2026-02-15-SWEEP`; verify: `git log --oneline -- docs/harness/TECH_DEBT_TRACKER.md src/ vtcode-core/`. |
| TD-003 | Features       | Session log export feature incomplete. Users cannot export conversation history in a portable format. | P2 | resolved | 2026-02-15 | `/share-log` now supports portable JSON and Markdown exports (`/share-log [json|markdown|md]`, alias `/export-log`) and writes timestamped files in workspace root. |
| TD-004 | Features       | Agent teams implementation is experimental and incomplete. `docs/agent-teams.md` documents the API but implementation gaps remain (teammate lifecycle, error recovery). | P2 | open | 2026-02-15 | Stabilize core team coordination. See `docs/agent-teams.md`. |
| TD-005 | TUI            | Large event handler modules in `src/`. Some files likely exceed 500-line invariant. Hard for agents to navigate. | P2 | resolved | 2026-02-15 | Ref: `TD-005-2026-02-15-SWEEP`; verify: `find src -name '*.rs' -type f -exec wc -l {} + | sort -nr | awk '$1>500 {print $1\" \"$2}'`. |
| TD-006 | Security       | Security documentation scattered across 10+ files. | P2 | resolved | 2026-02-15 | Consolidated into `docs/security/` with unified index. |
| TD-007 | Testing        | PTY session management has low test coverage due to inherent difficulty of testing terminal interactions. | P2 | in-progress | 2026-02-15 | Added `tmux` testing pattern in `.agent/workflows/pi-tui-test.md` for automated TUI verification. |
| TD-008 | Testing        | MCP OAuth flow and provider lifecycle lack integration tests. | P2 | in-progress | 2026-02-16 | Added startup-failure and partial-provider-failure coverage in `vtcode-core/tests/mcp_startup_timeout_test.rs`, plus existing lifecycle tests (`tests/fixtures/mock_mcp_server.py`) and CLI OAuth guardrails (`mcp login/logout` unsupported). Remaining gap: full OAuth callback-flow integration once implementation lands. |
| TD-009 | Documentation  | Tree-sitter language support optimization. | P3 | resolved | 2026-02-25 | Optimized tree-sitter integration: removed heavy language grammars in favor of LLM-native understanding. Retained tree-sitter-bash for critical shell safety. Updated `docs/protocols/LANGUAGE_SUPPORT.md`. |
| TD-010 | Configuration  | Individual config field documentation is partial. Not all `vtcode.toml` fields have descriptions. | P3 | resolved | 2026-02-15 | Added schema-generated field reference: `docs/config/CONFIG_FIELD_REFERENCE.md` (generated via `scripts/generate_config_field_reference.py`, sourced from `vtcode-config` schema). |
| TD-011 | Harness        | Harness documentation review. | P2 | resolved | 2026-02-16 | Full harness review conducted. Updated `QUALITY_SCORE.md` scoring rubric and aligned priority actions with active debt. |
| TD-012 | Codebase       | Legacy output patterns in tools may violate the new Agent Legibility invariant. | P2 | resolved | 2026-02-15 | Applied `badlogic-pi-mono` patterns for code quality and git safety. All new docs follow legibility invariant. |
| TD-013 | Codebase       | Structured logging audit. Legacy `println!` may exist in non-TUI crates. | P2 | resolved | 2026-02-15 | Audited crates and replaced `println!`/`eprintln!` with `tracing` macros in `vtcode-core` agent runner and registry modules. |
| TD-014 | Tooling        | Custom lints with remediation not yet implemented in CI. | P2 | resolved | 2026-02-15 | CI now runs custom invariant checks with remediation guidance: `scripts/check_large_files.py` and `scripts/check_markdown_location.py`. |

---

## How to Add a New Item

1. Assign the next sequential ID (`TD-NNN`).
2. Fill in all columns. Description should be specific enough for an agent to act on.
3. Set priority based on impact (see priority levels above).
4. Set status to `open`.
5. Set Last Reviewed to today's date.
6. Add notes with pointers to related docs, exec plans, or code locations.

## How to Resolve an Item

1. Update status to `resolved`.
2. Update Last Reviewed date.
3. Add a `Ref:` token in Notes (for example `PR-123`, `COMMIT-<sha>`, `PLAN-...`) plus one verification command (`git log`, test command, or invariant check).
4. Keep the row in the table for historical reference (do not delete).

## Review Cadence

- **P0 items**: review daily until resolved.
- **P1 items**: review weekly.
- **P2–P3 items**: review bi-weekly.
- **All items**: full table review monthly. Archive `resolved` items older than 3 months to a separate section.
