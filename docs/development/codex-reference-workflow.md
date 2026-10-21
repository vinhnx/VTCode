# DeepWiki grounding and Codex reference workflow

Start with repository instructions, relevant local source/tests, and project
memory. Routine edits and self-contained questions do not require DeepWiki.

Use DeepWiki only for complex or hard problems, especially questions about
OpenAI/Codex behavior where upstream evidence can materially affect a decision.
Use `vinhnx/vtcode` as the reference baseline and `openai/codex` for upstream
comparison. Examples include difficult cross-module runtime failures, unclear
architecture, or conflicting local and upstream contracts. Reuse relevant prior
research; a new user message does not by itself justify another query.

This is repository development guidance, shared through the root
[AGENTS.md](../../AGENTS.md) and imported by [CLAUDE.md](../../CLAUDE.md).

## Inspect and research

1. Read repository instructions, relevant skills, code, tests, and matching
   `.vtcode/memory/` entries. For a reported session failure, inspect persisted
   events, trajectory logs, and checkpoints to establish what happened.
2. For a complex/hard problem that needs DeepWiki, ask a focused question using
   [vinhnx/vtcode](https://github.com/vinhnx/vtcode) as the baseline and
   [openai/codex](https://github.com/openai/codex) for the relevant upstream behavior.
   A shared question may name both repositories in one query; use separate focused
   questions when their evidence needs differ. Preserve repository attribution.
   If an answer omits evidence that matters to the decision, fill that specific
   gap with a focused follow-up or direct source inspection. Do not restart the
   entire research pass merely to obtain coverage for its own sake.
3. Verify actionable claims against direct upstream source and tests at a pinned
   commit. DeepWiki explanations are discovery aids; distinguish their claims
   from verified behavior. Use `gh api repos/openai/codex/...` or a scoped source
   checkout if needed. Read relevant primary OpenAI research or guidance when it
   addresses the problem.
4. If DeepWiki MCP or upstream fetch is unavailable, fall back to a local
   `openai/codex` checkout. Resolve it from `$CODEX_REF_DIR`, defaulting to
   `~/developer/helpers/codex` only when that variable is unset:

   ```bash
   CODEX_REF_DIR="${CODEX_REF_DIR:-$HOME/developer/helpers/codex}"
   git -C "$CODEX_REF_DIR" fetch origin main
   git -C "$CODEX_REF_DIR" rev-parse origin/main
   ```

   Read source at that commit, for example
   `git -C "$CODEX_REF_DIR" show origin/main:codex-rs/...`, and cite the resolved
   commit. Point `$CODEX_REF_DIR` at any clone of `openai/codex`; contributors can
   set it in their shell or in a gitignored `.vtcode/`-local file rather than
   relying on the author's path.
5. Record source URLs, commit, source paths, observed behavior, and limits. If no
   source is available, state the gap and proceed using local evidence; do not
   describe unverified upstream behavior as confirmed.

## Compare and apply

Use the [read-and-apply skill](../../.agents/skills/read-and-apply/SKILL.md) for
external resources. Aggregate returned answers by claim, keeping each source,
path, and revision attached. Deduplicate agreements, resolve conflicts using
source evidence, and distinguish verified facts from inference. Compare each
applicable pattern with current VT Code code:

| Question | Required evidence |
| --- | --- |
| What local problem does the pattern address? | Current source, regression, or persisted session evidence |
| Is it already implemented? | Existing modules, helpers, tests, and architectural decisions |
| What can transfer safely? | Verified upstream behavior and explicit VT Code invariants |
| How will improvement be verified? | Focused regression or reproducible before/after measurement |

Rank confirmed issues as critical, high, medium, or low using their reachable
impact. Mark unverified suggestions, already-covered behavior, and out-of-scope
ideas separately; do not manufacture findings or port an upstream pattern merely
because it exists. Record a compact decision table: claim, source, local evidence,
severity or applicability, and action (apply, retain existing, or skip).

Validate proposed changes against repository guidelines, existing patterns,
public interfaces, and the original task's constraints. Implement confirmed gaps
using the smallest complete DRY/KISS change. Reuse existing helpers and modules.
Use relevant skills such as `rust-skills`,
`agentic-testing`, and `audit-module-agents`; add provider-specific skills when
provider work requires them. Continue from research through implementation and
verification when the user requested an improvement, unless they requested only
research or a plan.

After applying and verifying the applicable approaches, resume the work that
prompted the queries. Carry the validated findings and decision table forward as
enriched context; do not end at a research summary or ask again for already-given
authorization. Preserve the original mode and scope: research does not authorize
implementation for a read-only or planning-only request. When no new issue is
confirmed, retain the existing behavior and continue the task without speculative
changes. Refresh research only when a material evidence gap or changed premise
requires it.

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
