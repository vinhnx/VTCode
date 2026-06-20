change modes switching order, Build → Duck → Plan → Auto (auto last/most guarded); add trust gate to interactive auto; fix lifecycle tool policy hole; unify plan concept; unify prompt placement; centralize advertisement snapshot and add action-level masking for multi-action tools.

===

Perform a detailed audit of the vtcode modes and subagents, focusing specifically on the 'plan', 'build', and 'auto' modes. Identify any existing blockers, technical issues, or workflow inefficiencies. Propose an optimized system architecture and outline a validation process to ensure that each mode functions correctly and achieves its intended objective.

===

```

VT Code Modes & Subagents Audit

How it works today

Three independent mechanisms govern agent behavior, with no unifying layer:

╭──────────────────────────────────────────────────────────────╮
│ 1. PRIMARY AGENT MODES  (SubagentSpec, mode: Primary|All)      │
│    build · auto · plan · duck   — cycled via Tab/Shift+Tab     │
│    carries: prompt, tools allow/deny, permissions, model       │
│    enforced: primary_agent_allows_tool + permission eval       │
╰──────────────────────────────────────────────────────────────╯
╭──────────────────────────────────────────────────────────────╮
│ 2. PLANNING WORKFLOW  (planning_active session state)          │
│    toggled by /plan ONLY (not by selecting `plan` agent)       │
│    enforced: SafetyGateway + planning_workflow_checks          │
╰──────────────────────────────────────────────────────────────╯
╭──────────────────────────────────────────────────────────────╮
│ 3. FULL AUTO  (automation.full_auto + --full-auto/--auto)      │
│    primary agent (default `auto`) + enable_full_auto(allowlist)│
│    + skip_confirmations, gated by workspace-trust              │
╰──────────────────────────────────────────────────────────────╯

Built-in specs ([subagents.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-config/src/subagents.rs#L544-L627)): build = Ask/all-tools; auto = Auto/all-tools; plan = Deny+allow:[read] / tools:[unified_search, unified_file], mode All; duck = Deny / same readonly tools.

Blockers & issues (verified)

B1 — Two disconnected "plan" concepts. The plan primary agent (Tab) and the /plan planning workflow are separate systems with separate enforcement and different allow-lists. Selecting plan does not start the workflow; /plan does not switch agent. The docs even disclaim this: "/plan … is not a session state selector" ([interactive-mode.md:175](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/docs/user-guide/interactive-mode.md#L174-L178)). Worse, the plan primary is more restrictive than /plan (only unified_search/unified_file, no readonly exec), so the two "planning" experiences diverge.

B2 — Two disconnected "auto" concepts with different safety postures. Interactive Tab→auto uses PermissionDefault::Auto with no workspace-trust gate and no allow-list, while --full-auto requires trust + [automation.full_auto].allowed_tools ([auto.rs](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/cli/auto.rs#L34-L111)). Same name, very different blast radius. Related: vtcode exec silently auto-elevates the workspace to FullAuto trust before the trust check ([exec/prep.rs:146-165](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/cli/exec/prep.rs#L146-L165)) — audit if strict trust is an invariant.

B3 — Lifecycle tools bypass all policy, including explicit denies. is_subagent_lifecycle_tool forces spawn_agent, spawn_background_subprocess, send_input, wait_agent, resume_agent, close_agent to always be allowed/advertised, even when listed in disallowed_tools — locked in by [test:645-674](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/primary_agent.rs#L645-L673). In plan/duck, spawn_agent is advertised to the model but then denied at exec by the readonly permission default ⇒ wasted turns. It also conflates "manage existing child" (wait/close, safe) with "spawn new work" (a real policy hole). disallowed_tools is not authoritative.

B4 — Advertisement ignores permissions (unified TUI path). apply_primary_agent_policy_to_tool_snapshot filters only by tools/disallowed_tools, never effective permissions ([request_builder.rs:268-281](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_processing/llm_request/request_builder.rs#L268-L281)). The AgentRunner path does check advertised permissions, so the two paths disagree. Multi-action tools (unified_file) leak: advertised whole in read-only modes even though edit/write actions will be denied.

B5 — plan can't ask clarifying questions. Plan's allow-list lacks request_user_input (and readonly perms only allow semantic read), while the planning workflow explicitly permits it.

B6 — Cycle order is alphabetical ([support.rs:1036-1082](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/session/interaction_loop_runner/support.rs#L1071-L1083)) ⇒ auto, build, duck, plan. From default build, Shift+Tab lands on the riskiest mode (auto). Ordering depends on names, not safety posture.

B7 — Inconsistent prompt placement. Unified TUI injects active-agent instructions as a user message ("Active Primary Agent Runtime State") ([request_builder.rs:600-700](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/agent/runloop/unified/turn/turn_processing/llm_request/request_builder.rs#L612-L700)), while AgentRunner puts them in the system prompt ([execute.rs:265-295](file:///Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/core/agent/runner/execute.rs#L265-L295)). User-role instructions carry weaker authority and can be overridden by later user content.

Proposed architecture

Keep SubagentSpec and ThreadEvent authoritative (per AGENTS.md); add one derived policy layer, not a new config subsystem.

 SubagentSpec (persona)  planning_active   automation.full_auto   FeatureSet  trust
        │                     │                   │                   │         │
        ╰──────────────┬──────┴───────────┬───────┴─────────┬─────────┴────┬────╯
                       ▼                   ▼                 ▼              ▼
              ╭───────────────────────────────────────────────────────────────╮
              │  EffectiveSessionMode  →  EffectiveToolPolicy (single source)   │
              ╰───────────────────────────────────────────────────────────────╯
                  │              │                │                 │
                  ▼              ▼                ▼                 ▼
          tool snapshot   provider allow    prompt mode-block   exec admission
          (TUI + Runner share one path)

1. Unify plan: one enter_planning_mode() entered by /plan, NL trigger, and selecting plan agent. Selecting another primary during planning updates the "execution target after approval" instead of silently dropping planning. Single allow-list (fix the stale /plan help text too).
2. Unify auto: activating built-in auto applies the full-auto envelope (trust requirement + allowed_tools), not bare PermissionDefault::Auto. If trust is missing, prompt; if declined, keep prior agent. "Auto-approve everything" becomes an explicit custom agent, never the built-in.
3. Split lifecycle tools: spawn/new-work (spawn_agent, spawn_background_subprocess, send_input, resume_agent) obey tools/disallowed_tools/permissions and are hidden in read-only modes; only wait_agent/close_agent (cleanup of existing children) may bypass.
4. One advertisement path: TUI reuses the AgentRunner effective-policy snapshot (feature gates + primary policy + permissions + full-auto allow-list). Add action-level masking for multi-action tools (unified_file/unified_exec).
5. One prompt renderer emitting active-agent instructions as system/developer-priority context in both paths.
6. Explicit cycle order build → duck → plan → auto (auto last/guarded); custom agents sorted after built-ins.

Suggested order of work (low-risk first): B3 → B6 + auto-trust guard → B7 → B1 → B4/centralization → action masks.

Validation plan

Unit — built-in specs (vtcode-config): plan/duck expose request_user_input when interactive and readonly perms allow it; auto spec alone does not imply unrestricted full-auto.

Unit — effective policy matrix: for each mode assert advertised vs denied tools — spawn_agent hidden+denied in plan/duck; wait_agent/close_agent behavior explicit; disallowed_tools=["spawn_agent"] respected; auto-untrusted activation fails leaving agent unchanged.

Unit — TUI snapshot parity: primary_plan_snapshot_does_not_advertise_spawn_agent; unified_request_builder_and_agent_runner_snapshots_agree; readonly mode masks unified_file mutating actions.

Unit — exec permission posture: plan/duck → unified_file:read allow, :write/apply_patch/spawn_agent deny; planning workflow → .vtcode/plans/* write allow, source write deny, task_tracker allow; auto → global deny wins, outside-allowlist deny, untrusted activation fails.

Integration — session/TUI: /plan on and selecting plan agent yield identical state (is_planning_active()==true, header=plan, prompt block says planning); selecting build during planning keeps planning active; approval restores execution primary; Tab order build→duck→plan→guarded auto.

Prompt tests: active-agent instructions appear once, in system/developer context (both paths); planning/full-auto/request-user-input flags in the prompt match the tool snapshot.

CLI: --full-auto defaults to auto; explicit primary honored but allow-list still applies; untrusted workspace blocks --auto; verify exec does not silently mark the workspace full-auto-trusted if strict trust is intended.

---

This is an analysis/proposal deliverable — I made no code changes. The findings B1–B7 are all verified against the source (citations above) and cross-checked with the oracle. Want me to implement the low-risk fixes first (B3 lifecycle split, B6 cycle order + auto-trust guard, B7 prompt placement)?
```

===
