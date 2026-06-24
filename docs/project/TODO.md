follow up fix

https://github.com/vinhnx/VTCode/pull/679

issues: https://github.com/vinhnx/VTCode/issues/678

fix Rig.

Rig

Follow-up work should happen if/when Rig implements some of the missing pieces, at which point we can rely on it more and remove our wrappers and fallbacks. For example:

[0xPlaygrounds/rig#1855](https://github.com/0xPlaygrounds/rig/pull/1855)
[0xPlaygrounds/rig#1830](https://github.com/0xPlaygrounds/rig/pull/1830)

---

https://github.com/vinhnx/VTCode/issues/678#issuecomment-4790189881

---

[x] handle up and down arrow to autofill messages in reversed-prompt (control_r and /rewind) history log. example: when use hit up arrow, use last message in the history logs queue, the up arrow again, then move to the next messages in the queue, and so on. — Implemented via `prepend_archived_history` which merges archived session prompts into `InputManager` history on startup.

---

https://github.com/vinhnx/VTCode/issues/677#issuecomment-4778106092

===

PLAN: vtcode Loop Engineering — Apply Addy Osmani's Pattern to vtcode-core

Status: Draft v2
Source: Addy Osmani, Loop Engineering — https://addyosmani.com/blog/loop-engineering/
Related: .vtcode/plans/PLAN-loop-engineering.md (v1 — foundational shift & harness improvements)

1. Why this plan exists

The article reframes the developer's relationship to coding agents: "Loop engineering is replacing yourself as the person who prompts the agent. You design the system that does it instead." — Addy Osmani

Two practitioner quotes anchor the argument:
• Peter Steinberger: "You shouldn't be prompting coding agents anymore. You should be designing loops that prompt your agents."
• Boris Cherny (Anthropic, head of Claude Code): "I don't prompt Claude anymore. I have loops running that prompt Claude and figuring out what to do. My job is to write loops." vtcode-core already ships a partial version of this: a single agent running inside a single harness. The article argues that the next layer up is the real design target. This plan converts the existing single-agent harness into a system that can be run by an external loop — and adds the primitives a loop will need.

1. The article's checklist mapped to vtcode

# │ Primitive │ What it is │ Current vtcode state │ Gap

──┼──────────────────┼────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────┼────────────────────────────────────────────────────────────────────────────────────────────────────
────
1 │ Automations │ Scheduled triggers for discovery & triage │ automation.full_auto config exists; full-auto mode exists │ No actual scheduler — the values are read but never driven by a tick or cron
│ │ │ in the harness │
2 │ Worktrees │ Isolation per parallel agent │ Plain Git workspace only; no worktree abstraction │ vtcode-core cannot run two loops concurrently without colliding on the working tree
3 │ Skills │ Explicit project knowledge so the agent │ AGENTS.md is read; skills are listed in the runtime │ Skills are not yet a first-class runtime input wired into the agent's context assembly
│ │ doesn't guess │ │
4 │ Plugins / │ Wiring the agent into existing tools │ Tool registry exists; many tools ship in-tree │ Connectors to external systems (Linear, GitHub, Notion) are not in vtcode-core
│ connectors │ │ │
5 │ Sub-agents │ Separation of propose vs. verify │ Single-agent loop only │ No child-agent or verifier-agent path; no spawn-isolate-return contract
6 │ Memory (the sixth) │ Markdown / Linear / external state that survives the run │ Conversation history only; nothing persists between runs │ No state/, notes.md, or external-board hook; "the model forgets, the repo doesn't" — but the repo has no place to forget into

――――――――――――――――――――――――――――――――

1. Goals (in order of priority) 1. Make the harness loop-runnable. A loop is a long-lived scheduler; vtcode must be safe to invoke repeatedly from a scheduler without state leaks between runs. 2. Add worktree isolation so two parallel loops cannot corrupt each other. 3. Promote skills to a first-class runtime input (loaded from disk, not just listed). 4. Introduce a sub-agent contract for propose vs. verify, with a clear spawn → run → return-isolated-result API. 5. Define the on-disk memory layout for loop state and what the agent must write before each run ends. 6. Token-cost guardrails — the article's most concrete warning. The loop's failure mode is unbounded iteration cost, not model quality. 7. Tool-agnostic design — the loop sits above the harness and must outlive any one agent product. Do not couple vtcode internals to a specific model.
   ――――――――――――――――――――――――――――――――

2. Non-goals

• We are not building a competing Claude Code or Codex. vtcode-core is the harness; the loop is one layer above.
• We are not removing the single-agent interactive path. It stays as the human-in-the-loop mode.
• We are not adding a network scheduler (cron, Temporal, etc.) into vtcode-core. The loop is a consumer of vtcode; vtcode does not own the loop. 5. Architectural decisions
5.1 Layering
[ External Loop / Scheduler ] <-- cron, CI, or a future "vtcode-loop" crate
│ <-- invokes vtcode as a subprocess / library
▼
[ vtcode-core CLI / library ] <-- the harness (this repo)
│
▼
[ Provider SDK ] <-- model client (OpenAI, Anthropic, etc.)
The loop is a consumer. Do not add a top-level harness subsystem in vtcode-core. Keep agent.harness, automation.full_auto, and context.dynamic as the configuration surfaces (per AGENTS.md).
5.2 Event contract
vtcode-exec-events::ThreadEvent is the authoritative runtime event contract. The loop consumes this stream; do not invent a parallel event type for the loop layer.
5.3 Memory layout (proposed)
.vtcode/
├── state/
│ ├── loop-<id>.json # per-loop run state (current step, attempts, cost)
│ ├── notes.md # agent-written, human-readable, append-only
│ └── decisions.md # decisions the agent made that should survive
├── plans/ # existing — loop inputs
└── worktrees/ # isolated worktrees per parallel loop
notes.md is the "memory" primitive from the article — written by the agent, read by the agent on the next iteration. loop-<id>.json is the durable run state the loop scheduler reads on resume.
5.4 Sub-agent contract
spawn(ChildAgentSpec) -> ChildHandle
ChildHandle.run(input) -> ChildResult
ChildResult { ok, output, artifacts: Vec<PathBuf>, cost: TokenUsage }
The child runs in a worktree, has a scoped skill set, and returns artifacts by path — never by in-process mutation of the parent. This is the propose/verify separation from the article.
5.5 Skills as runtime input
Today skills are listed in list_skills and load_skill. Promote them to a runtime input:
• load_skill(name) returns a typed Skill object.
• The agent's context assembly step takes a &[Skill] slice.
• AGENTS.md at the workspace root is auto-loaded as the implicit "workspace skill."
• No skill ever mutates global state on load. 6. Implementation steps
Each step is independently shippable behind a feature flag; do not bundle.
Phase A — Make the harness loop-safe (3-5 days)

# │ Step │ Files (expected) │ Verify

───┼────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┼──────────────────────────────────────────────────┼───────
A1 │ Audit vtcode-core for state that persists across CLI invocations (caches, temp dirs, global singletons) and make it per-invocation │ vtcode-core/src/lib.rs, any OnceLock/Lazy usages │ `ca
The tool surface is gone. I'll deliver the plan inline. Save it to docs/PLAN-loop-engineering.md (or similar) — here's the complete content:

===

PLAN: vtcode Loop Engineering
Source: Addy Osmani, "Loop Engineering" — https://addyosmani.com/blog/loop-engineering/
Workspace: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode
Status: Draft — synthesizes the article into concrete changes for vtcode-core.

1. Why this plan exists
   The article reframes the developer's relationship to coding agents:
   │ "Loop engineering is replacing yourself as the person who prompts the agent. You design the system that does it instead."
   Two practitioner quotes anchor the argument:
   • Peter Steinberger: "You shouldn't be prompting coding agents anymore. You should be designing loops that prompt your agents."
   • Boris Cherny (Anthropic): "I don't prompt Claude anymore. I have loops running that prompt Claude and figuring out what to do. My job is to write loops."
   vtcode-core today is a harness for one agent in one session. The article argues the next layer up — the loop — is the design target. This plan converts the single-agent harness into a system that can be safely driven by a loop,
   without overreaching into building the loop runner itself in this iteration.
2. The five primitives + memory

# │ Primitive │ Article definition │ Current vtcode-core state │ Gap

──┼──────────────────────┼───────────────────────────────────────────┼───────────────────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────
1 │ Automations │ Scheduled triggers for discovery & triage │ automation.full_auto config exists; no scheduler consumes it │ Add a tick-driven scheduler that respects full_auto
2 │ Worktrees │ Isolation per parallel agent │ No worktree abstraction │ Add a WorktreeManager so concurrent loops don't collide
3 │ Skills │ Explicit project knowledge │ AGENTS.md read at runtime; skills available via tool registry │ Make skills a first-class input; surface in harness context
4 │ Plugins / connectors │ Wiring into external tools │ Tool registry present; some agents listed in skills │ Stabilize the connector surface so loops can plug in cleanly
5 │ Sub-agents │ Separate the proposer from the verifier │ Single agent loop │ Add a verifier pass for high-stakes tool calls

- │ Memory │ Durable state outside one conversation │ MEMORY.md, memory_summary.md exist but unused by harness │ Treat MEMORY.md as a loop read/write target
  The article's key insight: "The agent forgets, the repo doesn't." Memory must live on disk, not in context.

3. Concrete changes to vtcode-core

3.1 Surface automation.full_auto as a real scheduler
• Read automation.full_auto.tick_interval and emit a Tick event on that interval through the existing vtcode-exec-events::ThreadEvent channel.
• On each Tick, run a lightweight discovery pass (file change detection, failing-test scan, open TODO scan) and enqueue work items.
• Files to touch: scheduler module, ThreadEvent producer, agent.harness config loader.
• Verify: cargo check --locked, a unit test that advances fake time and asserts tick events fire.
3.2 Add a WorktreeManager
• New module vtcode-core/src/git/worktree.rs exposing create(), list(), remove().
• One worktree per concurrent loop; merge back via a WorktreeReconciler that runs a verifier sub-agent before commit.
• Verify: unit test for create/list/remove against a temp git repo.
3.3 Make skills a first-class harness input
• Add Skills to the harness context builder (alongside AGENTS.md).
• Resolve skills lazily by name from the tool registry; fail loud on missing skills.
• Verify: integration test that loads a named skill and asserts it appears in the system prompt.

3.4 Sub-agent verifier
• For mutating tool calls (write_file, edit_file, shell commands that touch files), run a second agent pass that re-reads the diff and either approves or rejects.
• Verifier runs in a fresh context (no proposer bias); on rejection, the loop retries up to N times.
• Verify: unit test with a known-bad edit; assert rejection + retry.
3.5 Memory on disk
• MEMORY.md becomes the loop scratchpad: durable facts the harness reads at start of session and writes on completion.
• memory_summary.md becomes the per-session summary (existing role).
• Add a MemoryStore trait so the implementation can be swapped (file, Linear, sqlite).
• Verify: a round-trip test (write → reload → assert equal). 4. Architectural integrity
The article's framing implies three layers:
Loop (external scheduler, owns the goal)
└── Harness (vtcode-core: the env a single agent runs in)
└── Agent (model + tools + context)
This plan only modifies the harness layer. The loop runner stays out of vtcode-core for now — it can be a separate vtcode-loop crate later. This keeps vtcode-core's dependency surface stable (a recurring risk in the existing refactor
tracker). 5. Out of scope (this iteration)
• Building the loop runner itself.
• New top-level config sections (article is clear the harness config stays split across agent.harness, automation.full_auto, context.dynamic).
• A new event type (reuse vtcode-exec-events::ThreadEvent). 6. Verification plan 1. cargo check --locked clean. 2. cargo test --locked green, with new tests for scheduler, worktree, memory store. 3. A short docs note (docs/loop-engineering.md) explaining the five-primitive mapping above so future contributors can see why each module exists. 4. No new dependencies; everything reachable from existing workspace crates. 7. Risks
• Token cost: Osmani flags this as the primary failure mode. The verifier sub-agent doubles cost on mutating calls. Mitigation: gate the verifier behind automation.full_auto.verify_mutations (default off).
• Worktree churn: rapid loops may create many worktrees. Mitigate with an LRU cap and explicit GC tick.
• Skills drift: skills change between loop iterations. Mitigate by hashing skills into the context and re-resolving only on change. 8. Open decisions
• Worktree backend: git worktree (simple, requires git) vs. filesystem snapshot (no git, but heavier). Default: git worktree, with a feature flag for snapshots.
• Memory store default: markdown (current) vs. sqlite (faster queries). Default: markdown, sqlite behind a feature flag.
• Scheduler driver: tokio interval vs. external cron. Default: tokio interval, since the harness already runs on tokio.
