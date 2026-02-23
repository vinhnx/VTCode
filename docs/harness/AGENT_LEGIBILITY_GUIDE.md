# Agent Legibility Guide

In VTCode, "legibility" means that information is easily parseable by both AI agents and humans. We favor mechanical structure over aesthetic prose.

## Core Rules

1. **Tables Over Prose**: Use tables for comparisons, status reports, and multi-item lists.
2. **YAML for Metadata**: Use YAML frontmatter or code blocks for structured metadata.
3. **Remediation is Mandatory**: Every error or violation report MUST include a `Remediation` instruction.
4. **Outcome First**: Lead with the result. Do not hide the "what" at the bottom of a "how" narrative.

## Examples

### 1. Status Reporting

**Bad (Prose-heavy)**:
I successfully updated the controller.rs file to handle the new variants. I also modified the tests.rs file to include the missing imports, which fixed the compilation error. Finally, I ran cargo check and it passed.

**Good (Structured Table)**:
| Component | File | Change | Outcome |
|-----------|------|--------|---------|
| Controller | `controller.rs` | Updated `SteeringMessage` handling | Refined steering logic |
| Tests | `tests.rs` | Added `TaskOutcome` import | Fixed compilation error |
| Validation | N/A | Ran `cargo check` | **PASSED** |

### 2. Error Reporting

**Bad**:
The file `src/main.rs` is too long (600 lines). Please fix it.

**Good**:
**Violation**: File `src/main.rs` exceeds 500-line invariant (602 lines).
**Remediation**: Split `src/main.rs` into focused submodules. Extract logical sections into separate files and re-export from `mod.rs`.

## Why It Matters

Structured information survives "context loss" better. If an agent picks up a task mid-way, it can scan a table 10x faster than reading through conversational history.

## Active Monitoring

Design outputs for human oversight, not just approval. Users should be able to monitor the "pulse" of the agent and interrupt only if necessary.

1. **Step-Level Visibility**: Update your plan or task tracker after every verification step, not just at the end of a phase.
2. **Intervention Hooks**: When performing a dangerous or high-risk operation, briefly explain the safety guard you have in place (e.g., "Running in Plan Mode first to audit the diff").
3. **Loop Transparency**: If you hit a repetition guard or tool stall, explain exactly what the root cause is before attempting a pivot.

**Example (Monitoring Friendly)**:
```
• [x] Update schema.rs (outcome: added UserStats table)
• [>] Run migrations (verification: checking pg_dump)
  └ Still waiting for DB lock... (30s)
```
