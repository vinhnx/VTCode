look into removing some of the internal surfaces now that Rig 0.40 was released and can be used directly, especially for OpenAI. There might also be quirks with the new tool surfaces but those should be easy quick patches

https://github.com/0xPlaygrounds/rig/releases/tag/v0.40.0

--

keep the plan crisp and not over-engineer,

==

Refined Plan: Eliminate token overhead in VTCode
Goal
Drive the default, out-of-the-box first-request token overhead (system prompt + tool schemas + instructions) below 12k tokens, and keep per-turn overhead growth bounded, while preserving capability. Fix the same mechanisms the article blames: oversized tool schemas, unstable cache prefixes, heavy subagent bootstraps, and redundant history.
Scope
Files and crates likely to change:

- vtcode-core/src/tools/handlers/session_tool_catalog.rs — deferred tool logic
- vtcode-core/src/tools/registry/builtins.rs — tool count cap
- vtcode-config/src/core/tools.rs, provider.rs, prompt_cache.rs, agent.rs — defaults
- src/agent/runloop/unified/turn/turn_processing/llm_request/prompt_assembly.rs — prompt assembly
- src/agent/runloop/unified/context_manager.rs — cached system prompt
- src/agent/runloop/unified/incremental_system_prompt.rs — incremental builder
- src/agent/runloop/welcome.rs — first-message scaffolding
- vtcode-core/src/prompts/system.rs — base prompt composition
- vtcode-core/src/prompts/guidelines.rs — runtime tool sections
- vtcode-core/src/core/agent/runner.rs or subagent setup — child agent config
- docs/config/CONFIG_FIELD_REFERENCE.md, docs/tools/TOOL_SEARCH.md, docs/development/EXECUTION_POLICY.md — docs
- Crate-level AGENTS.md files for affected crates

---

Phase 1 — Measure first, fix second
1.1 Add a first-request token budget test
Create a test in vtcode-core that builds a default session tool catalog and system prompt and asserts:

- System prompt text ≤ 8k tokens
- Visible builtin tool schemas ≤ 2k tokens
- Total first-request prefix ≤ 12k tokens with no MCP
- With 5 simulated MCP servers, total prefix grows by ≤ 25% (because deferral should kick in)
  1.2 Add per-request telemetry
  Emit a ThreadEvent or metrics snapshot containing:
- system_prompt_tokens
- tool_schema_tokens
- instruction_file_tokens
- message_history_tokens
- cache_read_tokens, cache_write_tokens, cache_miss_tokens
- subagent_bootstrap_tokens (when spawning)
  Use existing vtcode-core::metrics and vtcode-exec-events::ThreadEvent infrastructure.

---

Phase 2 — Tool schema tax (biggest win)
2.1 Lower the deferred-tool threshold
Change DIRECT_TOOL_EXPOSURE_THRESHOLD in session_tool_catalog.rs from 100 to 15 (matching MAX_LLM_VISIBLE_BUILTIN_TOOLS). When the catalog exceeds the builtin cap, deferral becomes active.
2.2 Add a token-based deferral guard
Even if the tool count is below the threshold, if the estimated tool-schema token count exceeds e.g. 4_000 tokens, force deferral. Add a helper estimate_tool_schema_tokens(tools).
2.3 Default client_tool_search = true for non-hosted providers
For providers without hosted tool search (Gemini, local, etc.), default client_tool_search to true so they also benefit from deferred loading. Keep Anthropic/OpenAI on their hosted path.
2.4 Tighten MCP tool descriptions

- Already capped at 512 chars (MCP_TOOL_DESCRIPTION_MAX_LEN). Verify this is applied in all modes.
- Also compact parameter schemas for MCP tools when ToolDocumentationMode is not Full.
  2.5 Audit core tool schema sizes
  Review the 15 visible builtin tools and ensure Progressive mode keeps their total description budget close to 1.2k tokens. Add a regression test.

---

Phase 3 — Cache prefix stability
3.1 Make first-message scaffolding cache-stable

- Move volatile session_addendum content (language summary, guideline highlights) out of the cached system prompt prefix, or keep it under a trimmable section.
- Make build_prompt_addendum respect agent.trim_system_prompt and the max_system_prompt_tokens budget.
  3.2 Harden cache-friendly shaping
- Ensure render_environment_addenda only emits temporal context when shaping is disabled, and that working directory is moved to the trailing volatile section when shaping is enabled.
- Verify the incremental system prompt cache key includes everything that affects the prefix, and nothing that changes per-turn.
  3.3 Reduce cache-breakers in subagent section
- Only include subagent descriptions if the subagent count is small; otherwise summarize or defer to list_skills.
  3.4 Add byte-stability test
  Hash the system prompt + tools prefix across two identical turns and assert equality.

---

Phase 4 — Subagent bootstrap cost
4.1 Lightweight default subagent profile
Create a default “subagent” runtime profile that:

- Uses SystemPromptMode::Minimal and ToolDocumentationMode::Minimal
- Carries only the default core tools plus the agent’s declared tools
- Excludes MCP tools unless explicitly requested
- Uses a smaller default model unless the spec overrides
  4.2 Apply the profile automatically
  In subagent setup (compose_subagent_instructions / child config builder), apply the lightweight profile when the agent spec does not explicitly request inherit_parent: true.
  4.3 Add subagent bootstrap token test
  Assert that a default subagent bootstrap is ≤ 40% of the parent bootstrap.

---

Phase 5 — History and tool-result compaction
5.1 Enable tool-result clearing by default
tool_result_clearing.enabled currently defaults to false. Enable it to clear old tool results from context.
5.2 Tighten auto-compaction
Review the auto-compaction thresholds to ensure long conversations compact aggressively enough.
5.3 Suppress redundant reasoning blocks
If reasoning/thinking blocks are carried in history, strip them during compaction unless the user explicitly opted to keep them.
5.4 Reduce repeated tool-call loops

- Keep max_tool_loops default at 0 (unlimited) is fine, but ensure max_repeated_tool_calls and max_consecutive_blocked_tool_calls_per_turn prevent churn.
- Review continuation policy to avoid unnecessary “one more check” turns.

---

Phase 6 — Lazy loading of optional capabilities
6.1 Lazy MCP connection
Do not connect all configured MCP servers at session startup. Connect on first use, or only when the tool is in the active catalog. This avoids paying schema cost for unused servers.
6.2 Lazy skill discovery
Skills are loaded only when used, but verify that skill metadata is not injected into the prompt until needed.
6.3 Lazy IDE context injection
Ensure editor context is only injected when the IDE provides it and the user has enabled it.

---

Phase 7 — Configuration and defaults
7.1 Review defaults

- Keep system_prompt_mode = Default (good balance).
- Keep tool_documentation_mode = Progressive (good balance).
- Consider adding a new “lean” preset that users can opt into.
- Keep max_system_prompt_tokens = 8000.
  7.2 Add config validation warnings
  Warn the user at startup if:
- MCP tool count exceeds the deferred threshold
- Estimated tool schema tokens exceed the budget
- System prompt exceeds max_system_prompt_tokens

---

Phase 8 — Documentation and module AGENTS.md
8.1 Update docs

- docs/config/CONFIG_FIELD_REFERENCE.md — new defaults
- docs/tools/TOOL_SEARCH.md — deferral behavior
- docs/user-guide/getting-started.md — how to audit token cost
- docs/development/EXECUTION_POLICY.md — cache-friendly execution guidance
  8.2 Audit module AGENTS.md
  Use the audit-module-agents skill for:
- vtcode-core
- vtcode-config
- vtcode-mcp
- vtcode (binary)
  Update any crate-local guidance that is now stale.

---

Verification checklist

- [ ] cargo nextest run -p vtcode-core
- [ ] cargo nextest run -p vtcode-core -E 'test(token|prompt|tool|defer|mcp|subagent|cache)'
- [ ] ./scripts/check-dev.sh --test
- [ ] ./scripts/check-dev.sh --lints
- [ ] cargo fmt --check
- [ ] audit-module-agents skill passes for affected crates

---

Recommended execution order

1. Phase 1 (measurement) so we can prove the wins.
2. Phase 2 (tool schema) — biggest impact.
3. Phase 4 (subagents) — second biggest.
4. Phase 3 (cache stability) — makes 2 and 4 cheaper.
5. Phases 5–7.
6. Phase 8.

===

Diagnosis: Does VTCode have the same issue?
Short answer: VTCode is already designed to avoid most of the overhead the article describes, but the same failure modes still exist under common real-world configurations.
The article identifies five root causes:
Article finding VTCode current state
33k system prompt vs 7k VTCode defaults to 8k max system prompt tokens, with Default mode ~6–7k, Progressive tool docs ~1.2k, and a 15-visible-tool cap.
Tool schema bloat Tool count capped, descriptions compacted, MCP capped at 512 chars, and deferred loading exists.
Cache re-writes / instability Anthropic/OpenAI tool-search deferral is enabled by default; prompt cache is keyed; cache-friendly shaping exists.
Instruction-file multiplier instruction_max_bytes / project_doc_max_bytes default to 16 KiB (~4k tokens).
Subagent multiplier Subagents inherit parent config and tool catalog; no lightweight default profile.
So the two concrete areas where VTCode repeats the same pathology are:

1. MCP/tool schema tax is not aggressively deferred — the 100-tool threshold means “small” MCP configs are still loaded eagerly.
2. Subagent bootstrap cost is not reduced — children inherit the full parent payload.

---

Plan to fix
Phase 1 — Establish baseline and observability

- Add a telemetry/metrics point that records the per-request token breakdown:
    - system prompt tokens
    - tool schema tokens
    - instruction file tokens
    - message history tokens
    - cache hit/miss/write counts
- Wire this into the existing SessionStats / ToolCatalogCacheMetrics so we can validate before/after.
- Add a small regression test that asserts the first request payload (system prompt + tools) stays under a configurable budget, e.g. 12k tokens in default config with no MCP.
  Phase 2 — Reduce tool schema tax
- Lower DIRECT_TOOL_EXPOSURE_THRESHOLD from 100 to something like 15–20 (matching the visible builtin-tool cap), so adding any MCP server immediately triggers deferred loading.
- Add a token-based fallback: even if tool count is below the threshold, if estimated tool-schema tokens exceed a budget (e.g. 4k), force deferral.
- Enable client_tool_search by default for providers without hosted tool search (Gemini, etc.), so non-Anthropic/OpenAI runs also benefit from deferred loading.
- Ensure the deferred-tools summary appended to the system prompt is deterministic and cache-friendly (it already is via BTreeMap grouping, but add a regression test).
- Add a regression test that with 5 simulated MCP servers, the first request tool schema is reduced vs. eager mode.
  Phase 3 — Improve cache stability and first-message scaffolding
- Review session_bootstrap.prompt_addendum (language summary, guideline highlights, workflow hint) and make it trimmable under the system-prompt budget, or move volatile parts out of the cached prefix.
- Ensure cache_friendly_prompt_shaping puts all volatile runtime context (working directory, temporal context, etc.) at the end of the system prompt so the prefix is byte-stable.
- Add a regression test that hashes the system prompt and tool prefix across two identical turns and asserts they are byte-identical.
  Phase 4 — Reduce subagent bootstrap cost
- Add a lightweight default subagent profile that:
    - Uses a smaller default model (Haiku/GPT-4-mini) unless explicitly overridden.
    - Carries only the default core tools plus whatever the agent spec explicitly requests.
    - Uses a shorter system prompt (e.g. Minimal mode) by default.
- Update compose_subagent_instructions and build_child_config to apply this lightweight profile when the agent spec does not explicitly request full parent tooling.
- Add a test that verifies subagent bootstrap token count is materially lower than parent bootstrap.
  Phase 5 — Documentation and config guidance
- Update docs/config/CONFIG_FIELD_REFERENCE.md and docs/tools/TOOL_SEARCH.md with the new defaults and guidance.
- Add a short user-facing doc explaining how to audit first-request token cost.
- Run the audit-module-agents skill for affected crates (vtcode-core, vtcode-config, vtcode-mcp) to update their local AGENTS.md files if needed.
  Verification
- cargo nextest run -p vtcode-core (existing tests)
- cargo nextest run -p vtcode-core -E 'test(token|prompt|tool|defer|mcp)' (new tests)
- ./scripts/check-dev.sh --test

---

Tradeoffs

- Lower deferral threshold: saves tokens but may add one extra round trip the first time a deferred tool is needed. The net cost is usually positive because deferred schemas are loaded only when needed rather than on every request.
- Lightweight subagent profile: may reduce capability for agents that truly need the full catalog. Mitigation: explicit opt-in in the agent spec keeps the full catalog.
- Default client_tool_search = true: changes model behavior for Gemini/etc. users; should be rolled out with clear config docs and a fallback.

--

check Vtcode plan mode

"The plans dir is empty. Let me present the plan inline (since I can't write files in planning mode)
The plans dir is empty. Let me present the plan inline (since I can't write files in planning mode).

------------------------------------------------------- Info -------------------------------------------------------
Tool execution completed, but the model follow-up failed (transient — may resolve on retry). Output above is valid
.

---

• Plans dir exists but empty. I need to write the plan file via apply_patch, but apply_patch is disabled in
recovery mode. The recovery directive says: "do NOT emit any tool calls or tool-call markup". I need to output
the plan in-line using the `apply_patch` tool.

it seem this is the reason plan mode is not working, because it cannot write to the plans dir. Need to check if this is a permission issue or if the plans dir is not being created properly. If it's a permission issue, we may need to adjust the file system permissions or change the location of the plans dir to a writable location. If it's a creation issue, we need to ensure that the plans dir is created at startup if it doesn't exist.

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_663.json

===

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/memory/llm-perf-2026-07-15.md
