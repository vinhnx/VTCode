# Core Beliefs

Agent-first operating principles for VTCode development. These beliefs guide every design decision, code review, and architectural trade-off.

## 1. Humans Steer, Agents Execute

Humans define goals, set constraints, and review outcomes. Agents handle implementation, testing, iteration, and routine maintenance. In VTCode's context:

- **Humans** choose which provider to add, which tool abstraction to expose, which security boundary to enforce.
- **Agents** write the provider adapter, implement the trait, add tests, update docs, and open the PR.

Corollary: if an agent cannot complete a task autonomously from the information in this repository, the repository is missing context — fix the repo, not the agent prompt.

## 2. Repository as System of Record

All authoritative knowledge lives in the repository. Not in Slack threads, not in Google Docs, not in someone's head.

- Design decisions are recorded in `docs/harness/exec-plans/`.
- Architectural invariants are codified in `docs/harness/ARCHITECTURAL_INVARIANTS.md`.
- Quality assessments live in `docs/harness/QUALITY_SCORE.md`.
- Technical debt is tracked in `docs/harness/TECH_DEBT_TRACKER.md`.

If a decision isn't in the repo, it hasn't been made.

## 3. Progressive Disclosure

`AGENTS.md` is the map. `docs/` is the territory.

- AGENTS.md provides a table of contents: workspace structure, key commands, critical conventions, and pointers to deeper documentation.
- `docs/harness/` contains the operational knowledge base: beliefs, invariants, quality scores, execution plans.
- `docs/ARCHITECTURE.md` describes the system design.
- Domain-specific docs (MCP, subagents, security) live in their respective `docs/` subdirectories.

An agent should be able to orient itself from AGENTS.md alone, then drill into specific docs only when working in that domain.

## 4. Agent Legibility Over Human Aesthetics

When there's tension between "looks nice to a human reader" and "an agent can reliably parse and act on this," choose legibility.

- Prefer structured formats (tables, YAML frontmatter, consistent headers) over prose.
- Use mechanical patterns: consistent naming, predictable file locations, explicit cross-references.
- Avoid ambiguity: "≤500 lines per module" beats "keep modules reasonably sized."
- Error messages should include remediation instructions, not just descriptions.

## 5. Enforce Invariants, Not Implementations

Define what must be true. Let agents decide how to make it true.

- **Parse at boundaries**: validate inputs where they enter the system (API boundaries, config loading, tool argument parsing), not deep inside business logic.
- **Strict layer dependencies**: `types → config → core → tools → agent → TUI`. No reverse imports.
- **No `unwrap()`**: use `anyhow::Result` with `.with_context()`. This is a mechanical rule, not a suggestion.
- **No hardcoded model IDs**: use `docs/models.json`. Models change frequently.

The invariants are documented in `docs/harness/ARCHITECTURAL_INVARIANTS.md` and should be enforced by CI, not by code review.

## 6. Boring Technology

Prefer composable, stable, well-documented tools and libraries that are well-represented in LLM training data.

- `anyhow` for errors, `serde` for serialization, `tokio` for async, `clap` for CLI, `ratatui` for TUI.
- New dependencies require justification: what problem does this solve that existing deps cannot?
- Avoid clever abstractions. A straightforward `match` statement is better than a macro that saves 3 lines.
- Stable APIs compose better than "flexible" ones. Prefer concrete types over trait objects when the set of implementations is known.

## 7. Throughput-First Merge Philosophy

Corrections are cheap. Waiting is expensive.

- Merge working code quickly. Fix issues in follow-up PRs.
- A PR that adds a feature with a known minor issue is better than a PR that sits in review for days.
- Tech debt is acceptable if tracked (see `docs/harness/TECH_DEBT_TRACKER.md`).
- "Perfect" is the enemy of "shipped and iterable."

This does not mean lowering quality standards. It means: if the tests pass and the invariants hold, merge it.

## 8. Entropy Management

Codebases accumulate entropy. Fight it with golden principles and recurring cleanup.

- **Golden principles** are the small set of rules that, if followed, prevent most categories of bugs: layer deps, no unwrap, parse at boundaries, workspace scoping.
- **Recurring cleanup** is scheduled, not reactive. Documentation gardening, dead code removal, and dependency updates happen on a cadence, not when someone notices rot.
- The `docs/harness/TECH_DEBT_TRACKER.md` is the entropy ledger. Review it regularly.

## 9. Technical Debt as High-Interest Loan

Pay continuously, not in bursts.

- Every PR that touches a module should leave it slightly better than it found it (boy scout rule).
- Large "cleanup sprints" are a sign that continuous maintenance failed.
- Tech debt items have priorities (P0–P3) and are reviewed on a regular cadence.
- P0 debt blocks new feature work. P3 debt is tracked but tolerated.

When adding new features, budget 10–20% of effort for paying down debt in the area you're touching.

## 10. Uncertainty Transparency

Agents should recognize when they are uncertain and surface it early.

- **Clarification > Guessing**: Asking a clarifying question via `ask_user_question` is more valuable than a confidently wrong implementation.
- **Assumptions as Flags**: If a task has multiple interpretations, state your assumption clearly and proceed—but keep it visible so a human can steer if the assumption holds.
- **Fail Fast on Ambiguity**: Significant ambiguity in project requirements should trigger a pause for clarification rather than a deep dive into an incorrect logical branch.

## 11. Post-Deployment Observability

Pre-deployment evaluations are necessary but insufficient. Agent behavior is co-constructed by model capability, product design, and user trust.

- **Systematic Monitoring**: Build privacy-preserving infrastructure to monitor how agents are actually deployed and used in the wild.
- **Evidence-Based Design**: Use real-world usage patterns to iterate on tool abstractions and security boundaries.
- **Agent Trace**: Maintain a high-fidelity system of record for all agent contributions to ensure accountability and provenance.
