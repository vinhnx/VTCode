--

Collapse agent's thinking block by default in TUI. Make it configurable (Extended/Collapsed) via a new config option, and add a test to ensure that the default behavior is to collapse thinking blocks, and that the config option can override this behavior.

===

refactor and extract vtcode-core/src/core/agent/runner/tests.rs

===

check if vtcode has same issue and make a plan to fix

https://systima.ai/blog/claude-code-vs-opencode-token-overhead

https://news.ycombinator.com/item?id=48883275

---

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
- Carries only core tools: unified_search, unified_file, unified_exec, plus the agent’s declared tools
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
    - Carries only the core tools (unified_search, unified_file, unified_exec) plus whatever the agent spec explicitly requests.
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

- Lower deferral threshold: saves tokens but may add one extra round trip the first time a deferred tool is needed. The net cost is usually positive because the schema is paid once via unified_search rather than on every request.
- Lightweight subagent profile: may reduce capability for agents that truly need the full catalog. Mitigation: explicit opt-in in the agent spec keeps the full catalog.
- Default client_tool_search = true: changes model behavior for Gemini/etc. users; should be rolled out with clear config docs and a fallback.

===

Plan: Improve VT Code Launch Time
Status: drafted (planning workflow)
Created: 2026-07-13
Predecessor:
.vtcode/memory/startup-optimization-2026-06-21.md (
already removed 6 hotspots, 300-600 ms saved)
Goal: shave another measurable slice off vtcode --
version / first-paint time without breaking behavior.
Summary
A focused, follow-up to the 2026-06-21 pass. Most
remaining cost is in StartupContext::from_cli_args (
src/startup/mod.rs:81-200): several sequential awaits,
a redundant file read, an unconditional clone + async
init of the dotfile guardian, and a few always-on
initializers that could be parallelized or skipped on
short-lived commands. Targets: ~80-200 ms across
cold/warm paths, with one small binary-size win.
Steps (Action -> files/symbols -> verify) 1. Parallelize independent from_cli_args inits ->
src/startup/mod.rs StartupContext::from_cli_args (lines
81-200). Move the post-config-validated fan-out (
initialize_dot_folder, init_global_guardian,
vtcode_core::utils::session_archive::
apply_session_history_config_from_vtcode, vtcode_core::
telemetry::perf::initialize_perf_telemetry,
file/command/read-limits caches, initialize_gatekeeper)
into one tokio::try_join! (or join! where errors are
tracing::warn! only). Keep strictly sequential:
load_startup_config -> validate_startup_configuration -

> resolve_session_resume ->
> resolve_runtime_model_selection ->
> build_runtime_agent_config (they depend on each other).
> Verify: ./scripts/perf/baseline.sh latest then
> compare.sh; expect 20-60 ms shaved on cold path.

     2. Make dotfile-guardian init lazy/sync ->

vtcode-core/src/dotfile_protection/guardian.rs::init_gl
obal_guardian + src/startup/mod.rs:116. The OnceCell is
already idempotent, but the call site awaits and clones
the full DotfileProtectionConfig. Split into
ensure_compiled() (no I/O, builds the in-memory pattern
set) eagerly, and a deferred load_audit_log(). Callers
that only need is_protected_dotfile (the common hot
path) never touch disk. Verify: cargo nextest run -p
vtcode-core -E 'binary(/dotfile_guardian/)'; micro-
bench with cargo bench if a bench exists, otherwise
time vtcode --version 8x. 3. Cache ~/.vtcode/config.toml read in
determine_theme ->
src/startup/theme.rs::determine_theme +
src/startup/mod.rs call site. Currently always calls
load_user_config() (file read + serde) on every launch.
Add a short-lived mtime-keyed cache in load_user_config
(vtcode-core/src/utils/dot_config.rs:630) backed by
OnceLock<HashMap<PathBuf, (SystemTime, Arc<DotConfig>)>

> ; invalidate on write through the existing update\_\*
> helpers. Verify: 5-15 ms shaved; unit test for stale-
> cache invalidation.

     4. Skip non-essential inits for

command_skips_provider_auth paths ->
src/startup/mod.rs:81-200 + command_skips_provider_auth
(line ~460). Commands that skip auth (Login, Logout,
Auth, ToolPolicy, AppServer, Notify, Pods, Schedule)
don't need perf telemetry, file/command caches,
gatekeeper, or session-archive config. Wrap the fan-out
in if !command_skips_provider_auth(args.command.as_ref(
)). Verify: ./scripts/check-dev.sh --test (existing
validation_tests already exercise the auth-skip
predicates); cargo nextest run -p vtcode -E 'binary(
/dispatch/)'. 5. Defer Copilot auth probe when not used ->
src/startup/mod.rs:140-156
resolve_runtime_provider_auth ->
vtcode-llm/src/copilot/auth.rs::probe_auth_status (and
the related resolve_runtime_provider_auth body for
copilot branch). probe_auth_status resolves a command,
may probe the local copilot CLI, and can hit the
network via the auth-source detection. For non-copilot
providers, short-circuit at the top of the match (
already does), but for the copilot branch the work runs
even when --print "..." will fail on missing auth
anyway. Move the copilot probe behind a tokio::spawn
whose result is consumed only when selection.provider =
= "copilot" AND we actually need an API key. Verify:
time vtcode --print "hi" --provider openai (8 runs);
expect 50-150 ms shaved on Copilot hosts.

6.  Drop a2a-server from default features (verify
    current state) -> Cargo.toml:288 and Cargo.toml:298.
    The 2026-06-21 note claims this was done; confirm
    default = [...] does not include a2a-server and that
    the comment line at :298 is the only reference. If
    already removed, skip; if not, move it to a non-default
    feature. Verify: cargo metadata --format-version=1 --no
    -deps | jq '.packages[] | select(.name=="vtcode") | .
    features'; ./scripts/check-dev.sh --lints to confirm no
    warning regressions. 7. Defer cleanup_old_temp_spools off the critical
    path -> src/main.rs run() (the post-
    resolve_startup_context block; same shape as the
    existing background preflight). Currently runs inline
    before cli::dispatch; on cold FS this can block 50-200
    ms. Wrap in tokio::spawn. Verify: time vtcode --version
    with cold ~/.vtcode/tmp; expect ~50-100 ms shaved cold. 8. Add a startup-perf budget to the perf harness ->
    scripts/perf/baseline.sh + .vtcode/perf/. Currently
    captures a single startup_ms for vtcode --version;
    extend with a first_user_io_ms measure that runs vtcode
    chat (or a --print "noop") and times up to the first
    byte on stdout. This makes regressions visible. Verify:
    ./scripts/perf/baseline.sh latest produces the new
    field; compare.sh diffs it. 9. Document the change ->
    docs/development/performance.md (after the "Local
    Workflow" section). Add a short "Startup budget"
    subsection listing the new measurements and the
    parallelization/lazy-init pattern. Update
    .vtcode/memory/startup-optimization-2026-06-21.md to a
    "v2" entry with the new numbers, OR add a new dated
    entry under .vtcode/memory/. Per AGENTS.md "Every new
    major feature must update docs" — a measurable perf
    change is a feature. Verify: render the page; cargo
    nextest run -p vtcode --bin doctest (if applicable) or
    visual review.

    Validation
    • ./scripts/check-dev.sh --test — fast gate + tests
    on changed crates (vtcode, vtcode-core).
    • ./scripts/check-dev.sh --lints — clippy clean (CI
    uses -D warnings).
    • ./scripts/check-dev.sh --workspace — full workspace
    check before PR.
    • ./scripts/perf/baseline.sh latest then compare.sh —
    capture before/after numbers; commit JSON to
    .vtcode/perf/ for review.
    • cargo nextest run -p vtcode -E 'binary(/dispatch/)'
    and -p vtcode-core -E 'binary(/dotfile_guardian/)' —
    targeted regressions.

===

check and fix vtcode plan mode

"Now I have a clear plan format precedent. Let me write the proposed plan
to a new file.
Tool budget exhausted. I'll synthesize and present the final plan inline
per the Planning workflow contract.
Tool budget exhausted. I have enough information to finalize the plan in
the response. Here is the proposed plan based on my research.
――――――――――――――――――――――――――――――――
The tool budget is exhausted. I'll synthesize the final plan from the
evidence already collected. Below is the proposed plan.
――――――――――――――――――――――――――――――――
""

the agent always error at plan mode summarization proposal and then it stop and exhaused.

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_647.json
