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
| TD-001 | Documentation  | Documentation sprawl: 649+ files in `docs/`, many stale or redundant. No deprecation policy, inconsistent naming, no clear hierarchy. See `QUALITY_SCORE.md` Documentation domain (grade C-). | P1 | open | 2026-02-13 | Consolidate related docs, archive stale ones, establish naming convention and gardening cadence. |
| TD-002 | Codebase       | Duplicated logic across workspace crates. Common patterns reimplemented instead of shared via `vtcode-commons`. | P2 | open | 2026-02-13 | Audit for duplication in error handling, path validation, and config reading patterns. Extract to `vtcode-commons`. |
| TD-003 | Features       | Session log export feature incomplete. Users cannot export conversation history in a portable format. | P2 | open | 2026-02-13 | Design export formats (JSON, Markdown). Implement in `vtcode-core`. |
| TD-004 | Features       | Agent teams implementation is experimental and incomplete. `docs/agent-teams.md` documents the API but implementation gaps remain (teammate lifecycle, error recovery). | P2 | open | 2026-02-13 | Stabilize core team coordination. See `docs/agent-teams.md`. |
| TD-005 | TUI            | Large event handler modules in `src/`. Some files likely exceed 500-line invariant. Hard for agents to navigate. | P2 | open | 2026-02-13 | Split event handlers into focused submodules. See `QUALITY_SCORE.md` TUI domain (grade C+). |
| TD-006 | Security       | Security documentation scattered across 10+ files (`docs/SECURITY.md`, `docs/SANDBOX_*.md`, `docs/COMMAND_SAFETY_*.md`). | P2 | open | 2026-02-13 | Consolidate into a single `docs/security/` directory with clear index. |
| TD-007 | Testing        | PTY session management has low test coverage due to inherent difficulty of testing terminal interactions. | P2 | open | 2026-02-13 | Investigate mock PTY approach or integration test harness for `vtcode-bash-runner`. |
| TD-008 | Testing        | MCP OAuth flow and provider lifecycle lack integration tests. | P2 | open | 2026-02-13 | Add mock MCP server for integration testing. See `QUALITY_SCORE.md` MCP domain. |
| TD-009 | Documentation  | Tree-sitter language support matrix not documented. Users and agents cannot easily determine which languages are supported. | P3 | open | 2026-02-13 | Create `docs/LANGUAGE_SUPPORT.md` with supported languages and feature matrix. |
| TD-010 | Configuration  | Individual config field documentation is partial. Not all `vtcode.toml` fields have descriptions. | P3 | open | 2026-02-13 | Generate config field docs from `schemars` schema or add inline documentation. |

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
3. Add a note referencing the PR, commit, or exec plan that resolved it.
4. Keep the row in the table for historical reference (do not delete).

## Review Cadence

- **P0 items**: review daily until resolved.
- **P1 items**: review weekly.
- **P2–P3 items**: review bi-weekly.
- **All items**: full table review monthly. Archive `resolved` items older than 3 months to a separate section.
