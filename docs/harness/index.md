# Harness Engineering Knowledge Base

## Purpose

This directory establishes an agent-first operating environment for VT Code development. It implements the "harness
engineering" pattern: structured documentation designed to be consumed by AI agents as their primary knowledge source,
while remaining useful to human contributors.

The principle: if an agent cannot complete a task autonomously from information in this repository, the repository is
missing context — fix the repo, not the agent prompt.

## File Index

| File                                                       | Description                                                                                             |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| [index.md](index.md)                                       | This file. Entry point to the harness knowledge base.                                                   |
| [core_beliefs.md](core_beliefs.md)                         | Agent-first operating principles. The "why" behind every decision.                                      |
| [architectural_invariants.md](architectural_invariants.md) | Mechanical enforcement rules. Each invariant includes violation detection and remediation instructions. |
| [zen_alignment.md](zen_alignment.md)                       | Full (all-19) Zen principle mapping and staged enforcement rollout.                                     |
| [quality_score.md](quality_score.md)                       | Quality grading (A/B/C/D) for each VT Code domain across 5 dimensions.                                  |
| [exec_plans.md](exec_plans.md)                             | Execution plan methodology. Template and guidelines for self-contained design documents.                |
| [tech_debt_tracker.md](tech_debt_tracker.md)               | Known technical debt items with priority (P0–P3) and review cadence.                                    |
| [session_log_review.md](session_log_review.md)             | Review of planning handoff recovery, session-loop boundaries, and harness diagnostics.                  |

### Key Implementation Files

| File                                                             | Description                                                                 |
| ---------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `crates/codegen/vtcode-core/src/core/agent/evaluator.rs`         | Structured evaluator with scoring dimensions and hard thresholds.           |
| `crates/codegen/vtcode-core/src/core/agent/handoff.rs`           | Enriched handoff protocol with boundary status, test results, known issues. |
| `crates/codegen/vtcode-core/src/core/agent/harness_artifacts.rs` | Sprint contract and outcome verification artifact support.                  |
| `crates/codegen/vtcode-core/src/core/agent/orient.rs`            | Orient phase: gathers orientation context from external artifacts.          |
| `crates/codegen/vtcode-core/src/core/agent/progress_monitor.rs`  | Monotonic progress monitoring with stall detection.                         |
| `crates/codegen/vtcode-memory/src/progress.rs`                   | Durable progress ledger with handoff metadata.                              |

### Directories

| Directory               | Description                                                     |
| ----------------------- | --------------------------------------------------------------- |
| `exec-plans/active/`    | Execution plans currently being worked on.                      |
| `exec-plans/completed/` | Finished execution plans kept for reference and retrospectives. |

## Cross-References

| Document                 | Location                                  | Relationship                                                                        |
| ------------------------ | ----------------------------------------- | ----------------------------------------------------------------------------------- |
| AGENTS.md                | `AGENTS.md` (root)                        | Top-level map. Points agents to this directory for deeper context.                  |
| Architecture Guide       | `docs/architecture.md`                    | System design reference. Harness docs cover process; architecture covers structure. |
| Configuration Precedence | `docs/config/configuration_precedence.md` | Config invariants in `architectural_invariants.md` reference this.                  |
| Security Model           | `docs/security/security_model.md`         | Security invariants reference the security docs.                                    |
| MCP Integration          | `docs/mcp/mcp_integration_guide.md`       | MCP domain quality score references this.                                           |
| Zen Principle Mapping    | `docs/harness/zen_alignment.md`           | Additive enforcement mapping between Zen principles and VT Code controls.           |
| Process Hardening        | `docs/development/process_hardening.md`   | Security domain quality score references this.                                      |
| Provider Guides          | `docs/providers/provider_guides.md`       | LLM domain quality score references this.                                           |

## Navigation

```text
AGENTS.md (root)          ← Start here. The map.
  │
  ├── docs/harness/       ← You are here. Operating environment.
  │   ├── index.md
  │   ├── core_beliefs.md
  │   ├── architectural_invariants.md
  │   ├── zen_alignment.md
  │   ├── quality_score.md
  │   ├── exec_plans.md
  │   ├── tech_debt_tracker.md
  │   ├── session_log_review.md
  │   └── exec-plans/
  │       ├── active/
  │       └── completed/
  │
  ├── docs/architecture.md  ← System design
  ├── docs/config/          ← Configuration details
  ├── docs/security/        ← Security model
  └── docs/providers/       ← Provider setup
```

## Maintaining Freshness

### Doc-Gardening Cadence

| Frequency | Action                                                                                              |
| --------- | --------------------------------------------------------------------------------------------------- |
| Per PR    | Update docs touched by code changes. Boy scout rule applies.                                        |
| Weekly    | Review `tech_debt_tracker.md` P0–P1 items.                                                          |
| Bi-weekly | Review `tech_debt_tracker.md` P2–P3 items. Scan `exec-plans/active/` for stale plans.               |
| Monthly   | Review `quality_score.md` grades. Update "Last reviewed" dates. Archive old resolved debt items.    |
| Quarterly | Full audit of `docs/harness/`. Verify cross-references. Check that invariants match CI enforcement. |

### Staleness Indicators

A document is stale if:

- Its "Last reviewed" date is more than 3 months old.
- It references files, modules, or APIs that no longer exist.
- Its content contradicts current code behavior.

When you find a stale document, either update it or add a tech debt item to track the update.
