DRY and refactor and extract desktop monolith components for reuse. also check for repeated patterns in existing screens. check view.rs

Refactor VT Code desktop module for KISS, DRY, lower token use, and fewer failures.

Follow `AGENTS.md` and existing VT Code architecture. Prefer small surgical changes, reuse existing helpers, remove duplication, and avoid new abstractions unless they clearly simplify the code.

Audit agent loop, tools, prompts, and safety paths. Find the top 5 refactors with concrete `file:line` evidence, then implement the highest-value low-risk one.

Focus on:

- duplicated logic or prompt rules
- repeated capability checks
- unnecessary allocations/scans
- dead or redundant paths
- overly complex control flow
- prompt text that can be shortened safely

Preserve behavior, tool permissions, sandbox/approval semantics, and `vtcode-exec-events::ThreadEvent`.

Output:
`problem | evidence | fix | benefit | verify`

Then implement #1, run the minimum relevant checks with `cargo nextest`, `cargo check --locked` + `RUSTFLAGS="-D warnings"`, and `./scripts/check-dev.sh` when needed. Inspect the final diff and fix regressions before finishing.

---

Before adding another subagent ask what problem it's solving. A bloated system prompt is often the issue.

Skills and tool search can fix that without splitting context across agents. My rule of thumb is to keep a subagent when the work is parallel, needs fresh context or one agent really can't pick the right tools.

Otherwise, fold it into the main loop, turn its prompt into a skill, run your evals and keep whatever wins.

https://claude.com/resources/articles/building-agents-when-to-fold-subagents-into-your-main-agent-loop

---
