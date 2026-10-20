# Planning validation

A plan becomes available for approval after structural validation and successful
persistence. It must include a summary, numbered implementation steps, validation
items, and assumptions or defaults. Complete plans do not require
`<proposed_plan>` wrappers. Approval commentary cannot replace the plan.

Each implementation step names concrete targets and its own verification:

```text
1. Update parser -> files: [src/parser.rs] -> verify: [cargo check --locked]
```

Use comma-separated verification entries. Commands and observable checks must
establish the step's result. Vague instructions such as `run checks` do not count.
An optional Markdown lint check cannot replace verification for Rust or other
non-Markdown targets. Targeted Git history or content inspection remains valid
for review tasks; a list of changed files does not verify implementation behavior.

Explicit scope-review steps may use Git status or diff checks when they name
concrete files and state an observable expectation limiting which files changed.
That expectation may appear in the action, outcome, verification, or validation
section. Prefer comparison against the captured workspace baseline so changes
that were already present when planning began remain accounted for:

```text
2. Review final scope -> files: [README.md] -> verify: [git status --porcelain] -> outcome: [only README.md differs from the captured baseline]
```

The validator reports scope-review and content-verification step counts
separately. Generated trackers store scope checks as `scope-review:` metadata,
while content checks use `verify:`. Scope checks are reviewed separately; they
cannot satisfy required functional checks. Scope review does not establish that
the README's links resolve, its contents are accurate, or a parser works.

Repair feedback identifies the specific failing step and the required correction.
Repairs preserve useful draft content and return a complete plan. If a scope step
has a concrete Git check but no observable scope expectation, repair that expectation;
do not discard the preceding implementation steps or substitute approval prose.

Headless planning uses the same validator and saves a complete plan through the
canonical plan/tracker writer before recording `awaiting_approval`. The returned
plan remains a draft for human approval; this never authorizes implementation.

If a response is empty, malformed, truncated beyond the existing rewrite
allowance, or violates a tool-free synthesis request, headless execution records
`awaiting_input` and stops automatic work. The canonical `planning.awaiting_input`
diagnostic includes retry, clarification, and model-selection actions. Plain text
includes labeled details; structured consumers retain the complete payload. Saved
checkpoints preserve consumed request allowances and public evidence across
resume. An unchanged awaiting-input checkpoint does not start another model
request. Private reasoning is classified by presence and never retained as draft
content or diagnostic details.

Focused regression suite:

```sh
cargo nextest run --locked -p vtcode-core -E 'test(planning_workflow)'
```
