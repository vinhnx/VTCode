# Harness Engineering Knowledge Base

## Purpose

This directory establishes an agent-first operating environment for VT Code development. It implements the "harness engineering" pattern: structured documentation designed to be consumed by AI agents as their primary knowledge source, while remaining useful to human contributors.

The principle: if an agent cannot complete a task autonomously from information in this repository, the repository is missing context — fix the repo, not the agent prompt.

## File Index

| File                                                       | Description                                                                                             |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| [INDEX.md](INDEX.md)                                       | This file. Entry point to the harness knowledge base.                                                   |
| [CORE_BELIEFS.md](CORE_BELIEFS.md)                         | Agent-first operating principles. The "why" behind every decision.                                      |
| [ARCHITECTURAL_INVARIANTS.md](ARCHITECTURAL_INVARIANTS.md) | Mechanical enforcement rules. Each invariant includes violation detection and remediation instructions. |
| [QUALITY_SCORE.md](QUALITY_SCORE.md)                       | Quality grading (A/B/C/D) for each VT Code domain across 5 dimensions.                                  |
| [EXEC_PLANS.md](EXEC_PLANS.md)                             | Execution plan methodology. Template and guidelines for self-contained design documents.                |
| [TECH_DEBT_TRACKER.md](TECH_DEBT_TRACKER.md)               | Known technical debt items with priority (P0–P3) and review cadence.                                    |

### Directories

| Directory               | Description                                                     |
| ----------------------- | --------------------------------------------------------------- |
| `exec-plans/active/`    | Execution plans currently being worked on.                      |
| `exec-plans/completed/` | Finished execution plans kept for reference and retrospectives. |

## Cross-References

| Document                 | Location                                  | Relationship                                                                        |
| ------------------------ | ----------------------------------------- | ----------------------------------------------------------------------------------- |
| AGENTS.md                | `AGENTS.md` (root)                        | Top-level map. Points agents to this directory for deeper context.                  |
| Architecture Guide       | `docs/ARCHITECTURE.md`                    | System design reference. Harness docs cover process; architecture covers structure. |
| Configuration Precedence | `docs/config/CONFIGURATION_PRECEDENCE.md` | Config invariants in `ARCHITECTURAL_INVARIANTS.md` reference this.                  |
| Security Model           | `docs/SECURITY.md`                        | Security invariants reference the security docs.                                    |
| MCP Integration          | `docs/mcp/00_START_HERE.md`               | MCP domain quality score references this.                                           |
| Subagents Guide          | `docs/subagents/SUBAGENTS.md`             | Subagent domain quality score references this.                                      |
| Process Hardening        | `docs/development/PROCESS_HARDENING.md`               | Security domain quality score references this.                                      |
| Provider Guides          | `docs/providers/PROVIDER_GUIDES.md`                 | LLM domain quality score references this.                                           |

## Navigation

```
AGENTS.md (root)          ← Start here. The map.
  │
  ├── docs/harness/       ← You are here. Operating environment.
  │   ├── INDEX.md
  │   ├── CORE_BELIEFS.md
  │   ├── ARCHITECTURAL_INVARIANTS.md
  │   ├── QUALITY_SCORE.md
  │   ├── EXEC_PLANS.md
  │   ├── TECH_DEBT_TRACKER.md
  │   └── exec-plans/
  │       ├── active/
  │       └── completed/
  │
  ├── docs/ARCHITECTURE.md  ← System design
  ├── docs/config/          ← Configuration details
  ├── docs/security/        ← Security model
  └── docs/subagents/       ← Subagent system
```

## Maintaining Freshness

### Doc-Gardening Cadence

| Frequency | Action                                                                                              |
| --------- | --------------------------------------------------------------------------------------------------- |
| Per PR    | Update docs touched by code changes. Boy scout rule applies.                                        |
| Weekly    | Review `TECH_DEBT_TRACKER.md` P0–P1 items.                                                          |
| Bi-weekly | Review `TECH_DEBT_TRACKER.md` P2–P3 items. Scan `exec-plans/active/` for stale plans.               |
| Monthly   | Review `QUALITY_SCORE.md` grades. Update "Last reviewed" dates. Archive old resolved debt items.    |
| Quarterly | Full audit of `docs/harness/`. Verify cross-references. Check that invariants match CI enforcement. |

### Staleness Indicators

A document is stale if:

- Its "Last reviewed" date is more than 3 months old.
- It references files, modules, or APIs that no longer exist.
- Its content contradicts current code behavior.

When you find a stale document, either update it or add a tech debt item to track the update.
