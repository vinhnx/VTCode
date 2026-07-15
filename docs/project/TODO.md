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

===

# LLM request / streaming hot-path pass (2026-07-15)

Third perf area after startup + runtime hot paths. Source: Explore-agent sweep of
`vtcode-core/src/core/agent/{runtime,request_plan,runner/execute}.rs`.

## Findings & disposition

### #1 HIGH — O(n²) full-text snapshot clone per streaming chunk [FIXED]

- `record_model_progress` (runtime/mod.rs `AgentRuntime`) + the test-only
  `StreamingLifecycleBridge::push_assistant_delta` both called
  `emit_assistant_snapshot` on **every** `OutputDelta` chunk.
- `emit_text_snapshot` (events/lifecycle.rs:671) clones the entire accumulated
  `state.text` each call → copying O(n) bytes O(n) times = O(n²).
- Worse: `emit_pending_lifecycle_events` → `emit_event` (runtime/mod.rs:626)
  `event.clone()`s every snapshot into `emitted_events` too, so each chunk was
  cloned **twice** (text clone + event clone), both O(n²).
- Fix: throttle output (and reasoning) snapshot emission with
  `MIN_OUTPUT_UPDATE_BYTES=1024`, `MAX_OUTPUT_UPDATE_EVENTS=64` (mirrors the
  existing reasoning throttle). Counters reset per turn in
  `run_turn_once_with_adapter`. Bounded event count ⇒ total copy = O(n), not
  O(n²). Final `complete_assistant_stream` always delivers full text, so UI/log
  correctness preserved. Added `assistant_len()` accessor on the emitter.
- Verified: clippy clean, runtime + harness_kernel tests pass (15 + 31).

### #3 MEDIUM — system prompt re-hash per turn [FIXED]

- `build_harness_request_plan` (request_plan.rs:49) called
  `stable_system_prefix_hash(&input.system_prompt)` every turn (full ~100KB scan).
- `RuntimePromptBundle` is already memoized (PROMPT_CACHE), so the hash is
  computed once at bundle build and stored as
  `system_instruction_prefix_hash`. Threaded via new
  `HarnessRequestPlanInput::system_prompt_prefix_hash: Option<u64>` (same pattern
  as existing `tool_catalog_hash`). Fallback recomputes when `None` (tests/bench).
- Production binary path (`src/agent/runloop/unified/turn/.../request_builder.rs`)
  already computes `stable_prefix_hash` once/turn for cache fingerprinting (line
  120); now passes it via `system_prompt_prefix_hash: Some(stable_prefix_hash)`,
  removing the duplicate recompute inside `build_harness_request_plan`.
- Verified: harness_kernel `request_plan_keeps_stable_prefix_hash` still passes;
  workspace `cargo check` + `cargo clippy` clean.

### #4 LOW — tool schema re-validation per turn [ALREADY SOLVED]

- `build_harness_request_plan` only re-hashes tool defs via
  `input.tool_catalog_hash.or_else(hash_tool_definitions)`. Production passes the
  precomputed `prompt_bundle.tool_snapshot.tool_catalog_hash`, so
  `hash_tool_definitions` runs only for non-bundle callers (tests). No change.

### #5 LOW–MEDIUM — tool-schema JSON re-serialization per wire request [LEFT, intrinsic-ish]

- `LLMRequest.tools` is already `Option<Arc<Vec<ToolDefinition>>>`; the per-turn
  cost is the provider `convert_request`/`prepare` re-serializing the definitions
  into provider-specific JSON every request. For large tool catalogs this is a
  few KB of serialization per turn (microseconds, not a bottleneck like #1/#2).
- Cheap win available: the catalog is stable per session and already fingerprinted
  by `tool_catalog_hash`. Memoize the serialized wire payload per
  `(provider_kind, tool_catalog_hash)` in a `Mutex<HashMap<(ProviderKey, u64),
Arc<Value>>>` inside the provider client, invalidated on catalog change. Only
  safe if the wire shaping depends solely on the definitions + provider (no other
  request-level field) — verify before caching.
- Left as optional; see plan below.

### #2 HIGH-rated but INVASIVE — double full-history clone per turn [LEFT, plan below]

- Two full-history clones per turn in `execute.rs`:
    - line 698 `messages: request_messages.into_owned()` — `prepare_responses_request_messages`
      (execute_helpers.rs:88) returns `Cow<'a, [Message]>`; `.into_owned()` deep-copies
      the whole history into `LLMRequest.messages` even in the common `Cow::Borrowed` case.
    - line 747 `let sent_messages = request.messages.clone()` — clones again so
      `set_previous_response_chain` (line 815) can keep a copy while the provider
      still holds `request.messages` (MiMo validates non-empty messages during stream).
- Real cost: history grows unbounded over a session (thousands of messages, MBs of
  text), cloned twice every turn. Fix requires an `Arc<Vec<Message>>` refactor of
  conversation-history ownership — broad, touches session store, compaction,
  subagents, threads, continuation. Deferred pending sign-off.

## Future improvement plan (post sign-off)

### Plan A — eliminate the double history clone (#2) via `Arc<Vec<Message>>`

Status: drafted. Goal: make per-turn history handling O(1) (Arc bumps) instead of
O(history) (two deep clones). Expected win: removes the dominant remaining
per-turn allocation for long sessions.

Steps (Action → files/symbols → verify):

1. Introduce a shared alias `pub type ConversationHistory = Arc<Vec<Message>>;`
   (vtcode-core `core/agent/types.rs` or `session/mod.rs`). Keep `Message` as is.
2. Change `AgentSessionState.messages` (session/mod.rs:31 `Vec<Message>` →
   `Arc<Vec<Message>>`). Append path currently `state.messages.push(...)` becomes
   `Arc::make_mut(&mut state.messages).push(...)` — O(1) when uniquely owned
   (common case), clones only when shared.
3. Change `LLMRequest.messages` (vtcode-llm/src/provider/request.rs:98
   `Vec<Message>` → `Arc<Vec<Message>>`). `serde` already supports `Arc<T>`
   Serialize/Deserialize, and `Default`/`Clone` become O(1); wire JSON unchanged.
4. Change `HarnessRequestPlanInput.messages` (request_plan.rs:27) →
   `Arc<Vec<Message>>`; `build_harness_request_plan` `Arc::clone`s into the request
   instead of moving. Update the 2 constructions (execute.rs:697,
   src/.../request_builder.rs:157) and the bench (agent_harness.rs:50).
5. Eliminate clone #1: in execute.rs:697, capture the source `Arc`.
   `prepare_responses_request_messages` already returns `Cow<'a,[Message]>` borrowing
   `&runtime.state.messages`; at the call site do
   `match request_messages { Cow::Borrowed(_) => Arc::clone(&runtime.state.messages),
Cow::Owned(v) => Arc::new(v) }` → no deep copy in the borrowed (common) case.
6. Eliminate clone #2: execute.rs:747 `request.messages.clone()` is now an O(1)
   Arc bump; `set_previous_response_chain` (session/mod.rs) takes the `Arc` by move.
7. Audit remaining `state.messages`/`messages: Vec<Message>` consumers:
   `core/threads.rs:55,64,96,177,248`, `core/agent/state.rs`, `subagents/types.rs:244,538,547`,
   continuation/compaction call sites, `replace_messages` (threads.rs:248). Use
   ast-grep to enumerate `messages` ownership transitions; switch appends/pushes to
   `Arc::make_mut` and ownership takes to `Arc` clones/moves.
8. Verify:
    - `cargo clippy --workspace` clean (CI `-D warnings`).
    - `cargo nextest run -p vtcode-core -E 'test(runtime) or test(harness_kernel) or test(session)'`
      and `cargo nextest run -p vtcode -E 'test(turn_processing)'` — responses-API
      continuation must still record identical sent messages (assert via a regression
      test on `set_previous_response_chain`).
    - Micro-bench: extend `vtcode-core/benches/agent_harness.rs` with a
      history-clone bench (build N requests from an M-message history; assert clone
      cost is flat as M grows). Capture before/after in `.vtcode/perf/`.

Risk: blast radius across session/compaction/subagent/thread ownership. Phase it:
A1 = steps 2–6 (request boundary + `LLMRequest`/`HarnessRequestPlanInput` Arc) with
`state.messages` still `Vec` but wrapped into an `Arc` only at the boundary
(deep-copies once into the Arc — partial win, low risk); A2 = step 7 (full
`state.messages` Arc) for the complete win. Recommend A1 as a safe first landing.

### Plan B — memoize tool-schema wire serialization (#5)

Status: drafted (optional, lower priority). Goal: skip per-turn re-serialization of
stable tool definitions.
Steps:

1. In the provider client (`vtcode-llm/src/provider/...` `convert_request`/`prepare`),
   add `Mutex<HashMap<(ProviderKey, u64), Arc<serde_json::Value>>>` keyed by provider
   kind + `tool_catalog_hash`.
2. On request build, if `tool_catalog_hash` present and cache hit, reuse the
   pre-serialized `tools` Value; else serialize, store, reuse.
3. Verify shaping depends only on defs+provider (no request-level field leaks into
   the `tools` array) before enabling — otherwise widen the cache key.
4. Verify: `cargo nextest run -p vtcode-core -E 'test(harness_kernel)'`
   (`tool_catalog_hash_matches_legacy_json_string_hash`,
   `request_plan_drops_empty_tool_catalog`); bench `agent_harness` shows flat
   tool-serialization cost across turns with a stable catalog.

## Net

Streaming output of length n now copies O(n) bytes instead of O(n²); per-turn
system-prompt hashing eliminated. Both are now bounded by constants, not response
size. Plans A/B above target the remaining O(history) per-turn clones and the
intrinsic tool re-serialization, respectively.
