# Building VT Code, a year in

[VT Code](https://github.com/vinhnx/vtcode) is a Rust terminal coding agent with Tree-sitter and ast-grep for structural
code intelligence, 26+ LLM providers, and a sandboxed execution model. Last December I wrote down
[five lessons from building it](https://huggingface.co/blog/vinhnx90/vt-code). Since then the project has reached
v0.154.0 and grown from a tight core into a workspace of roughly 30 crates. Some predictions held up. Some didn't. This
is the update, with the actual numbers this time.

![VT Code in action: a live terminal session with streaming responses, tool calls, and status line](https://raw.githubusercontent.com/vinhnx/VTCode/main/resources/gif/vtcode.gif)

The agent at work. Streaming turns, tool calls, and a status line tracking tokens, cost, and context pressure, all
inside the terminal.

## The model shelf, September 2026

The provider abstraction's real test is whether it absorbs the frontier as it moves. What the catalog holds today,
straight from `vtcode-config`'s model constants:

| Lab       | Frontier models in the picker                                               |
| --------- | --------------------------------------------------------------------------- |
| Anthropic | Claude Opus 5, Sonnet 5, Fable 5.1, Mythos 5.1                              |
| OpenAI    | GPT-5.6 (Luna, Sol, Terra), GPT-5.2 Codex, GPT-OSS 120B / 20B               |
| Google    | Gemini 3.8, 3.7, 3.6 Flash, 3.1 Pro                                         |
| xAI       | Grok 4.6, Grok 4.20 (reasoning, non-reasoning, multi-agent), Grok Build 0.1 |
| Moonshot  | Kimi K3, K2.7 Code                                                          |
| DeepSeek  | V4 Pro, V4 Flash                                                            |
| Z.AI      | GLM-5.3, GLM-5.2                                                            |
| Meta      | Muse Spark 1.1-1.3                                                          |
| Plus      | MiniMax, Qwen, StepFun, Xiaomi MiMo, NVIDIA Nemotron, Poolside Laguna       |

Twenty-six providers, plus OpenRouter and Merge Gateway as meta-routers, GitHub Copilot as a provider, and Ollama / LM
Studio / llama.cpp for local runs. None of these additions touched the agent loop. That's the abstraction working.

_Source:
[vtcode-config model constants](https://github.com/vinhnx/VTCode/tree/main/crates/codegen/vtcode-config/src/constants/models),
[docs/models.json](https://github.com/vinhnx/VTCode/blob/main/docs/models.json)._

## Benchmarks, with honest framing

The repo carries a repeatable benchmark setup (`make bench-humaneval`, plus MBPP and SWE-bench harnesses), and the
numbers taught me more than the features did.

On HumanEval, gpt-5-nano passed 155 of 164 tasks (94.5%) at 10.4s median latency and ~$0.10-0.30 per million tokens.
Gemini 3 Flash Preview passed 101 (61.6%) at 0.97s median, free tier. The gap between those rows is the whole argument
for provider diversity: the frontier model is 10x slower and not free, and most agentic turns (exploration,
summarization, tool selection) don't need it. Routing cheap turns to cheap models is worth more than any single model
upgrade.

The harness itself got measured too. A three-phase optimization pass on the provider layer, tracked with Criterion, cut
per-request heap allocations by ~30%, hot-path clones by 44%, and provider overhead by 23% on average. In agent
workflows where inference dominates at 1-5 seconds per turn, those microseconds don't matter to the user. They matter to
me: allocation discipline is a proxy for how much of the code someone has actually thought about.

| Benchmark | Tasks | What it tests                | VT Code uses it for    |
| --------- | ----- | ---------------------------- | ---------------------- |
| HumanEval | 164   | Code generation from scratch | Quick model validation |
| MBPP      | 974   | Basic Python programming     | Larger dataset         |
| SWE-bench | 2,294 | Real-world bug fixing        | Production readiness   |

One caveat: a HumanEval score through VT Code measures the harness plus the model, not the model alone. Tool formatting,
prompt assembly, and retry behavior all move the number. That's the point of measuring end to end.

_Sources: [benchmark summary](https://github.com/vinhnx/VTCode/blob/main/docs/benchmarks/SUMMARY.md),
[performance benchmarks](https://github.com/vinhnx/VTCode/blob/main/docs/benchmarks/performance_benchmarks.md),
[benchmark comparison](https://github.com/vinhnx/VTCode/blob/main/docs/benchmarks/BENCHMARK_COMPARISON.md)._

## The roadmap shipped

In the first article I described plans to extract `vtcode-llm` and `vtcode-tools` into standalone crates. That happened,
and went further than planned. The workspace now includes `vtcode-skills`, `vtcode-memory`, `vtcode-eval`,
`vtcode-safety`, `vtcode-mcp`, and others, each with its own `AGENTS.md` of crate-local conventions.

The main payoff wasn't the reuse I originally designed for. It was discipline. Once crate layering was enforced, the
dependency graph started catching design mistakes early: any change that would introduce a cycle meant the abstraction
was wrong. The build graph became a reliable first-pass reviewer, before any human looked at the diff.

```text
                 vtcode binary (CLI, TUI bootstrap)
                              |
                        vtcode-core
                  (agent loop, tools, prompts)
                    /      |         \
   satellites:  llm     memory     safety
                mcp     skills     eval
                a2a     indexer    webmcp
                    \      |         /
              common crates: exec-events,
                macros, tool-specs, config
```

_Source: [workspace layout in AGENTS.md](https://github.com/vinhnx/VTCode/blob/main/AGENTS.md#workspace)._

Dependencies point inward only. The satellites never depend on each other; they meet inside `vtcode-core`. When a PR
tried to make `vtcode-memory` aware of the TUI, the build said no, and it was right.

## The harness matters more than the model

This is the largest shift in how I think about the project. VT Code is a harness, not a wrapper. The model reasons; the
harness enforces progress. Concretely, three pieces of infrastructure:

First, a single runtime contract. Every event is a `ThreadEvent`, one enum covering `thread.started`, `turn.completed`,
`plan.delta`, and a dozen other variants, versioned as `VersionedThreadEvent` so the serialized schema evolves
explicitly. Session memory is an append-only log of those events with derived views on top, and trajectory export in
ATIF (Agent Trace Interchange Format) fell out of the same design.

Second, measurement. A dedicated eval crate runs each task k times and computes pass@k and pass^k, split into two
categories: capability (does it handle new things) and regression (does it still handle old ones). Outcomes are verified
against the environment, not model self-reports: a probe runs a shell command, checks a file exists, or verifies the git
tree is clean. Unmeasured intuition about agent quality is usually wrong; mine certainly was.

Third, loop engineering for long autonomous runs. Each sub-agent gets its own git worktree under `.vtcode/worktrees/` so
parallel runs don't collide. A separate read-only verifier re-reads the affected files and approves or rejects proposed
changes; it shares no context with the proposer, which matters, because agents grade their own work generously. Loop
state persists to disk, and a `SessionBudget` pauses or escalates when token costs cross a threshold, so a runaway loop
can't burn through an API budget overnight.

```text
 user --> [plan: read-only] --approved--> [agent turn]
                                            |
                                        tool call
                                            v
                                  [sandbox + policy gate]
                                            |
                              +-------------+-------------+
                              v                           v
                   (ThreadEvent log,             [verifier sub-agent:
                    append-only,                  fresh context,
                    ATIF export)                  read-only]
                              ^                    |       |
                              +----reject/feedback-+       |
                              +--------approve------+------>
                                                      [reviewed change]
```

_Sources: [vtcode-exec-events](https://github.com/vinhnx/VTCode/tree/main/crates/common/vtcode-exec-events)
(ThreadEvent + ATIF), [vtcode-eval](https://github.com/vinhnx/VTCode/tree/main/crates/codegen/vtcode-eval) (pass@k,
probes), [PLAN-loop-engineering.md](https://github.com/vinhnx/VTCode/blob/main/docs/project/PLAN-loop-engineering.md)
(worktrees, verifier, budgets)._

The verifier sees the diff and the files, not the proposer's reasoning. That's deliberate: reasoning is persuasive, and
persuasion is not verification.

## Context engineering got real numbers

The original article mentioned auto-compaction at "85% token budget." The real number is 90%, driven by actual
tokenization with Hugging Face's `tokenizers` library, tracked per component: system prompt, messages, tool results. The
decision ledger that survives compression holds at most 12 entries, each with a confidence score. Tool results over 8
KiB get spooled out of the prompt entirely; outputs over 10k lines show the first 5k and last 5k. PTY output is capped
at 8,000 tokens per turn with a 40 KiB byte fuse behind it, because token estimates can be wrong in both directions.

None of this is glamorous. All of it is the difference between an agent that works for a demo and one that works for a
hundred turns.

_Sources:
[context constants](https://github.com/vinhnx/VTCode/blob/main/crates/codegen/vtcode-config/src/constants/context.rs),
[context engineering guide](https://github.com/vinhnx/VTCode/blob/main/docs/context/context_engineering.md)._

## Provider governance

The `LLMProvider` trait now fronts 26 built-in providers, custom OpenAI-compatible endpoints, and local inference.
Adding a model means touching a constants file, `docs/models.json`, and a handful of enum mappings; tedious but never
architectural, which is what the abstraction was designed to buy.

One addition I didn't plan for: a `providers_whitelist` setting restricting which providers a workspace can use.
Flexibility is valuable until someone on a team accidentally routes code to an endpoint nobody approved. Workspace-level
config also can't introduce custom provider auth commands or endpoint overrides; only system and user config are trusted
for that. Governance turned out to be part of the abstraction's job.

## Security grew up

The original design had sandboxed execution and path validation. The current model is more specific:

```text
 model requests shell command
        |
        v
 tree-sitter-bash parse (pipes, &&, redirections split)
        |
        v
 for every subcommand:
        |
        +--> matches deny list/glob/regex? --yes--> BLOCKED + logged
        |
        +--> matches allow list/glob/regex? --no--> BLOCKED
        |
        v
 needs escalation? --yes--> human approval (scoped to this command)
        |                              |
        no                       denied v
        |                         BLOCKED
        v                       approved
  execute in sandbox <-------------+
```

_Source:
[COMMAND_SECURITY_MODEL.md](https://github.com/vinhnx/VTCode/blob/main/docs/development/COMMAND_SECURITY_MODEL.md)._

Anything unmatched is denied. Fail-closed, not fail-open. Shell commands aren't matched as text; they're parsed with
tree-sitter-bash so pipes, `&&` chains, and redirections are decomposed and every subcommand is checked individually.
Path validation is symlink-aware and rejects parent traversal and mutations targeting the workspace root itself.

The full model is five layers deep, each with its own failure mode:

| Layer               | What it does                             | What it blocks                              |
| ------------------- | ---------------------------------------- | ------------------------------------------- |
| Command allowlist   | Only explicitly allowed commands execute | `rm`, `sudo`, `docker`, unsandboxed `curl`  |
| Argument validation | Per-command flag allowlists              | Execution flags like `--pre`, `-exec`, `-e` |
| Workspace isolation | Path normalization and canonicalization  | `../` traversal, symlink escapes            |
| Sandbox integration | Filesystem isolation, network allowlists | Out-of-sandbox reads and writes             |
| Human-in-the-loop   | Approve once / for session / always      | Anything above its trust level              |

_Source: [SECURITY_MODEL.md](https://github.com/vinhnx/VTCode/blob/main/docs/security/SECURITY_MODEL.md)._

Approvals are scoped: "allow for session" lives in memory only, and cached approvals are keyed to the specific command
shape, not the tool. A granted `cargo test` does not become a granted `cargo anything`.

Most of these fixes came from real review findings, not paranoia. The operating principle: treat the boundary between
model output and the shell as adversarial by default, and convert every security bug into a regression test rather than
a one-off patch.

## Protocols beat integrations

Instead of bespoke integrations per editor or tool, VT Code adopted open protocols: MCP (stdio, HTTP, and child-process
transports), ACP for Zed, A2A for agent-to-agent communication, Agent Skills, and Agent Plugins, where repository
plugins are metadata-only and native loading requires explicit approval. The newest addition is the WebMCP bridge: an
authenticated pairing flow that lets a terminal session drive a browser editor through eight bounded tools.

![The VT Code WebMCP browser app: an editor surface with file tree, inspector, console, and write boundary panel](https://raw.githubusercontent.com/vinhnx/VTCode/main/resources/screenshots/webmcp/vt-webmcp-subtitle-02.png)

The WebMCP app. The browser side is a full editor surface (files, tree, inspector, console, write boundary), but every
mutation flows through the harness's tool contract.

![Pairing: the terminal shows a live code that expires in seconds; the browser accepts it with origin and expiry visible](https://raw.githubusercontent.com/vinhnx/VTCode/main/resources/screenshots/webmcp/vt-webmcp-subtitle-04.png)

Pairing is explicit and short-lived. The terminal displays a code, the browser shows the origin and expiry it's about to
trust, and nothing happens until a human clicks Trust.

![Draft review in the WebMCP app: the terminal proposes edits, the browser shows pending changes before they apply](https://raw.githubusercontent.com/vinhnx/VTCode/main/resources/screenshots/webmcp/vt-webmcp-subtitle-08.png)

Every browser-side write carries a SHA256 digest of the content it expects to replace. Stale digest, no write. The
browser is a client of the harness, not a peer.

Try the hosted app: [vinhnx.github.io/VTCode](https://vinhnx.github.io/VTCode/) (GitHub Pages reference client) or
[vtcode.vinhnx.chatgpt.site](https://vtcode.vinhnx.chatgpt.site/) (hosted demo). Pair from a running session with
`/webmcp pair https://vinhnx.github.io`.

Each protocol was cheaper to adopt than the one before because the tool and event contracts were already stable. That is
the architecture investment compounding.

_Source: [webmcp guide](https://github.com/vinhnx/VTCode/blob/main/docs/development/webmcp.md),
[vtcode-webmcp crate](https://github.com/vinhnx/VTCode/tree/main/crates/codegen/vtcode-webmcp)._

## Taste is a CI job

A smaller lesson: if a rule matters, encode it in CI. VT Code's checks enforce things most projects leave to reviewer
discretion. No `unwrap()` outside tests. WCAG AA 4.5:1 contrast for every built-in theme, validated by an actual test
suite, with `ui.minimum_contrast` if you want AAA's 7:1. Structured logging conventions, file length limits, dead-code
analysis, ast-grep lint rules. The project runs on edition 2024 with an MSRV of 1.93, and even the tree-sitter parse
tables' 2.4 MiB of binary weight is measured and documented.

If a rule can't be expressed as a check, it's worth asking whether it's a rule or just an opinion.

## The field caught up to the thesis

The most validating part of the last year was watching the industry's research agenda converge on things this project
already had. From Anthropic's engineering blog alone, in just the past few months:

- [Harness design for long-running application development](https://www.anthropic.com/engineering/harness-design-long-running-apps)
  (March 2026). The harness-over-model framing is now mainstream.
- [Building a C compiler with a team of parallel Claudes](https://www.anthropic.com/engineering/building-c-compiler)
  (February 2026). Parallel agent teams with isolation; the same idea as worktree-isolated propose/verify sub-agents, at
  terminal scale.
- [Quantifying infrastructure noise in agentic coding evals](https://www.anthropic.com/engineering/infrastructure-noise)
  (February 2026) and
  [Demystifying evals for AI agents](https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents) (January
  2026). Environment-based verification with capability/regression splits is what `vtcode-eval` does.
- [Claude Code auto mode](https://www.anthropic.com/engineering/claude-code-auto-mode) (March 2026) and the earlier
  [sandboxing work](https://www.anthropic.com/engineering/claude-code-sandboxing) (October 2025). Safer autonomy through
  containment rather than more prompts: the fail-closed policy chain and scoped approvals, built from the start.
- [Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp) (November 2025) and
  [Agent Skills](https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills) (October
  2025). Both protocols are first-class citizens in VT Code, alongside ACP and A2A.

I don't claim priority on any of these ideas; the field is small and everyone is reading everyone. I claim something
simpler: a one-person open-source project landed on the same answers as the best-funded agent teams, because the
constraints of the problem push everyone toward the same architecture. The constraints are the teacher.

## What I'd tell myself a year ago

The five original lessons still hold, but they've collapsed into one: an agent is infrastructure, not a demo. Models
will keep changing underneath you. The crate boundaries, the event contract, the sandbox, and the evals are what let you
swap models without starting over.

For anyone evaluating the tool rather than the philosophy, the recent additions I'm happiest with:

- **Graceful blocked-call recovery**: a blocked tool call gives the agent structured feedback and a path forward, and
  the TUI shows exactly what was refused and why.
- **Compact transcript review**: tool output collapses into per-call summaries you can expand on demand.
- **Bounded failure diagnosis**: repeated tool failures get diagnosed with a bounded retry budget instead of looping
  until the context window dies.
- **The `/plan` workflow**: a read-only planning agent produces a proposed plan, you approve it, and the build agent
  executes against a persisted task tracker. Planning and doing are different agents with different permissions.

If you're building your own agent, start with the architecture. The prompts are the easy part, and the first thing to go
stale.

## Build with me

VT Code is open source (MIT) and welcomes contributions: bug reports, provider additions, skills and plugins, docs
fixes, eval tasks, or just an issue describing a workflow that broke for you. The per-crate `AGENTS.md` files and the
eval harness exist so a new contributor can make a correct change without understanding the whole system. If a PR tries
something the architecture disagrees with, the build will tell you before I do.

- Code, issues, PRs: [github.com/vinhnx/VTCode](https://github.com/vinhnx/vtcode). Small focused PRs merge fastest.

You can find me at [vinhnx.github.io](https://vinhnx.github.io/), and elsewhere: [GitHub](https://github.com/vinhnx) ·
[Hugging Face](https://huggingface.co/vinhnx90) · [LinkedIn](https://www.linkedin.com/in/vinhnx) ·
[Twitter/X](https://twitter.com/vinhnx) · [YouTube](https://www.youtube.com/vinhnx90) ·
[Stack Overflow](https://stackoverflow.com/users/1477298/vinh-nguyen) ·
[Hacker News](https://news.ycombinator.com/user?id=vinhnx).

Project: [https://github.com/vinhnx/vtcode](https://github.com/vinhnx/vtcode)
