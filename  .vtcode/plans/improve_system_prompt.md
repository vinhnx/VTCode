# Improve System Prompt

This ExecPlan is a living document. Keep Progress, Surprises & Discoveries,
Decision Log, and Outcomes & Retrospective up to date as work proceeds.

Reference: `.vtcode/PLANS.md` for full specification.

## Purpose / Big Picture

Enhance the system prompt used by the VT Code agent to provide clearer instructions, better error handling guidance, and more context for users. The updated prompt should:
1. Clarify the agent's role and constraints.
2. Embed quick access to core docs.
3. Provide a concise troubleshooting flow.
4. Ensure alignment with architectural invariants.

## Progress

- [ ] Identify current system prompt location.
- [ ] Draft improved prompt text.
- [ ] Validate syntax and formatting.
- [ ] Update prompt file.
- [ ] Run a sanity check that the agent loads the new prompt.

## Surprises & Discoveries

(Document unexpected findings with evidence)

## Decision Log

- Decision: Use `docs/harness/AGENT_LEGIBILITY_GUIDE.md` as reference.
  Rationale: Central doc for prompt structure.
  Date: 2026-03-02

## Outcomes & Retrospective

(Will be filled after completion)

## Context and Orientation

Key files:
- `src/system_prompt.rs`: contains current prompt string.
- `docs/harness/AGENT_LEGIBILITY_GUIDE.md`: guide for prompt design.

## Plan of Work

1. Search for `system_prompt` definition.
2. Open and read the file.
3. Draft new prompt content.
4. Write new content back to file.
5. Run `cargo check` to ensure no syntax errors.
6. Verify agent loads prompt via `cargo run -- --prompt-test` (hypothetical).

## Validation and Acceptance

- `cargo check` passes.
- Running the agent shows the updated prompt in logs.
- No lint or style violations.
