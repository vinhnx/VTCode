# Codex reference workflow for VT Code internals

When asked to improve VT Code's internal system, research the corresponding
OpenAI/Codex implementation before designing the change. This applies to agent
loops and harnesses, tools and editing, prompts, context and history, provider
orchestration, sandboxed execution, MCP, and related runtime behavior. Keep the
research focused on the requested problem and the existing VT Code architecture.

This is repository development guidance, shared through the root
[AGENTS.md](../../AGENTS.md) and imported by [CLAUDE.md](../../CLAUDE.md).

## Inspect and research

1. Read repository instructions, relevant skills, code, tests, and matching
   `.vtcode/memory/` entries. For a reported session failure, inspect persisted
   events, trajectory logs, and checkpoints to establish what happened.
2. Use DeepWiki MCP to locate the corresponding implementation in
   [openai/codex](https://github.com/openai/codex). Ask a focused question such as
   “How does openai/codex implement patch mismatch recovery in codex-rs, and which
   source files and tests define its limits?”
3. Verify actionable claims against direct upstream source and tests at a pinned
   commit. DeepWiki explanations are discovery aids; distinguish their claims
   from verified behavior. Use `gh api repos/openai/codex/...` or a scoped source
   checkout if needed. Read relevant primary OpenAI research or guidance when it
   addresses the problem.
4. Record source URLs, commit, source paths, observed behavior, and limits. If
   upstream access is unavailable, state the gap and proceed using local evidence;
   do not describe unverified upstream behavior as confirmed.

## Compare and apply

Use the [read-and-apply skill](../../.agents/skills/read-and-apply/SKILL.md) for
external resources. Compare each applicable pattern with current VT Code code:

| Question | Required evidence |
| --- | --- |
| What local problem does the pattern address? | Current source, regression, or persisted session evidence |
| Is it already implemented? | Existing modules, helpers, tests, and architectural decisions |
| What can transfer safely? | Verified upstream behavior and explicit VT Code invariants |
| How will improvement be verified? | Focused regression or reproducible before/after measurement |

Implement confirmed gaps using the smallest complete DRY/KISS change. Reuse
existing helpers and modules. Use relevant skills such as `rust-skills`,
`agentic-testing`, and `audit-module-agents`; add provider-specific skills when
provider work requires them. Continue from research through implementation and
verification when the user requested an improvement, unless they requested only
research or a plan.

Preserve the authoritative `ThreadEvent` contract, harness configuration split,
sandbox and approval boundaries, mutation leases, conflict checks, rollback,
durable history, and unrelated work. Upstream patterns inform the design; they
must fit VT Code's requirements and existing public APIs.

## Attribute and verify

For adapted Codex patterns, cite a source URL pinned to a commit and the relevant
`codex-rs/...` path. Preserve copyright and license notices and document changes
for substantive copies under Apache-2.0 section 4. The repository's existing
attribution identifies Apache-2.0, copyright 2025 OpenAI. For substantive copied
code, update `scripts/templates/third-party-header.txt` and regenerate
`THIRD-PARTY-NOTICES` using `scripts/generate-notices.sh`.

Run focused locked `cargo nextest run` selections with a nonzero test count and
`./scripts/check-dev.sh` for runtime changes. Update compiled guidance and its
presence/budget tests when model-facing behavior changes. Documentation-only
updates need Markdown, link, and diff checks.

Report what was applied, what was already present or skipped, source evidence,
validation, and remaining limits. Separate deterministic fixture measurements
from live-provider results. Record durable findings in `.vtcode/memory/` and keep
this workflow in repository guidance.
