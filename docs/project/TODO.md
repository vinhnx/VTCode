
the user wants a comprehensive codebase review focused on:

Large files/modules → extract into smaller, composable pieces
Agent loops, tool calls, system prompts alignment
Autonomous safe tool execution
Modular design, separation of concerns
Reduce latency, improve reliability

Review and create a prioritized refactoring plan for the VT Code codebase. Focus on the largest files that need decomposition into smaller, composable modules:

PRIORITY 1 (3000+ lines - Critical):

vtcode-core/src/llm/providers/anthropic.rs (3799 lines)
vtcode-core/src/llm/providers/openai/provider.rs (3182 lines)
vtcode-core/src/tools/registry/mod.rs (3017 lines)

PRIORITY 2 (2000-3000 lines - High):
4. vtcode-core/src/config/models.rs (2577 lines)
5. vtcode-core/src/mcp/mod.rs (2505 lines)
6. src/acp/zed.rs (2505 lines)
7. vtcode-core/src/tools/file_ops/tool.rs (2503 lines)
8. vtcode-core/src/core/agent/runner.rs (2448 lines)
9. vtcode-core/src/llm/providers/openrouter.rs (2331 lines)
10. vtcode-core/src/tools/pty.rs (2103 lines)
11. src/agent/runloop/unified/turn/tool_outcomes.rs (2007 lines)

For each file, identify:

Logical boundaries for extraction
Shared abstractions that can be reused
Agent loop patterns that need alignment
Tool execution patterns needing consolidation
State management improvements
Error handling standardization
1) TL;DR

Refactor by extracting “pure” translation/parsing code first, then isolating IO + retries + telemetry, and finally untangling stateful orchestration (agent loop + tool registry + MCP). Enforce the AGENTS.md 500-line limit by moving each major responsibility into a dedicated module with a small, stable public API, and converge on one unified tool-execution pipeline and one unified provider HTTP/streaming stack.

Effort signal: L–XL overall (1–3w) if done safely + incrementally with tests/fixtures; each file can be decomposed in M–L (1–3h to 1–2d) slices.

---

2) Recommended approach (simple path): prioritized plan

Phase 0 (foundation, prevents churn) — S–M (<1h–3h)

Define module boundaries & target APIs first (even as Rust mod skeletons with pub(crate) items).
Add/expand golden tests for:
Provider request JSON bodies (Anthropic/OpenAI/OpenRouter).
Provider streaming event decoding.
Tool registry execution lifecycle (policy deny, loop detection, timeouts).
MCP initialization + tool discovery caching behaviors.
Introduce (or confirm) a single set of shared “core” abstractions (details below) before moving code.

Phase 1 (highest leverage latency/reliability wins) — L (1–2d)

Unify tool execution pipeline: runner ↔ tool registry ↔ tool outcomes.
Standardize provider HTTP + streaming + error mapping: Anthropic/OpenAI/OpenRouter.

Phase 2 (structural cleanup / separation-of-concerns) — L–XL

Decompose config/models catalog logic.
Decompose MCP client module.
Decompose PTY + file_ops.

---

3) Per-file decomposition plan (with the 6 requested dimensions)

Below, each file has:

A) Logical boundaries for extraction (modules)
B) Shared abstractions to reuse
C) Agent loop alignment points
D) Tool execution consolidation points
E) State management improvements
F) Error handling standardization

---

PRIORITY 1 (Critical)

1) vtcode-core/src/llm/providers/anthropic.rs (3799)

A) Logical boundaries (extract modules; each <500 lines)

anthropic/provider.rs: AnthropicProvider struct + LLMProvider impl surface only.
anthropic/request_builder.rs: LLMRequest -> AnthropicRequest(Value) conversion (system/messages/tools/tool_choice/thinking config).
anthropic/response_parser.rs: non-stream response parsing into LLMResponse (+ usage conversion).
anthropic/stream_decoder.rs: SSE/event-stream decoding into LLMStreamEvent (and reasoning trace extraction).
anthropic/prompt_cache.rs: TTL selection, beta header composition, breakpoint logic.
anthropic/minimax_compat.rs: resolve_minimax_base_url and any model-specific quirks.
anthropic/headers.rs: auth headers, beta headers, structured outputs toggles, tool-search toggles.
anthropic/tests/…: fixtures for request JSON and streaming sequences.

B) Shared abstractions to reuse

A provider-agnostic:
HttpRequestFactory (build reqwest request + headers)
ProviderStreamDecoder trait (input bytes → typed events)
ProviderRequestBuilder trait (LLMRequest → provider JSON)
UsageMapper helper (provider usage → internal usage)
Reuse existing providers/common more aggressively: base_url override, model resolution, prompt cache settings extraction.

C) Agent loop patterns needing alignment

Tool calling semantics: ensure tool call ids / names / arguments normalize identically to OpenAI/OpenRouter to avoid runner/tool_outcomes divergence.
Reasoning: Anthropic “thinking” blocks should map to a single internal representation used by runner display + tool outcomes.

D) Tool execution patterns to consolidate

Anthropic tool schema conversion should not live inline—move to shared llm/tools/schema.rs so tool definitions are stable across providers.

E) State management improvements

Keep AnthropicProvider mostly immutable:
config snapshot
http_client
base_url/model
Avoid hidden behavior based on env-vars deep inside; resolve config once in from_config / factory and store normalized values.

F) Error handling standardization

Replace ad-hoc format_*_error patterns with a shared:
ProviderHttpError { status, code, message, retryable, request_id }
ProviderParseError { context, body_excerpt }
Ensure every provider returns LLMError with:
retryable flag
normalized categories (timeout, rate_limit, auth, invalid_request, server_error, network, parse)

---

2) vtcode-core/src/llm/providers/openai/provider.rs (3182)

A) Logical boundaries

openai/provider.rs: OpenAIProvider public impl only.
openai/request_builder.rs: build request payloads (chat/responses API differences, json mode, tool choice).
openai/response_parser.rs: parse response → LLMResponse (including tool calls).
openai/stream_decoder.rs: SSE decoding, partial deltas → LLMStreamEvent.
openai/model_capabilities.rs: model-specific switches (supports reasoning, tools, structured outputs).
openai/error_mapping.rs: status/body → LLMError.
openai/tests/…: golden fixtures.

B) Shared abstractions

Same as Anthropic: ProviderRequestBuilder, ProviderStreamDecoder, ProviderErrorMapper.
Shared “tool call normalization” module that produces internal ToolCall consistently.

C) Agent loop alignment

Align “finish_reason” mapping and streaming finalization semantics with Anthropic so runner doesn’t need provider-specific conditionals.

D) Tool execution consolidation

OpenAI tool-choice/tool-call constraints should be represented in a shared internal “tool policy for LLM request” struct, not scattered.

E) State management

Ensure per-request options (timeouts, retries, streaming enabled) are parameters, not mutable provider state.

F) Error handling

Normalize OpenAI rate-limit / overloaded / context_length_exceeded into the same LLMErrorKind used elsewhere, with retry_after when available.

---

3) vtcode-core/src/tools/registry/mod.rs (3017)

This file is already a “module hub” but still contains a huge ToolRegistry implementation that mixes concerns.

A) Logical boundaries

tools/registry/registry.rs: ToolRegistry struct + constructor wiring only.
tools/registry/execution/mod.rs:
preflight.rs (policy checks, allow/deny/prompt, rate limiting, loop detection hook)
execute.rs (dispatch: builtin vs MCP vs PTY-backed)
postprocess.rs (normalize output, summarization/spooling, history recording)
tools/registry/mcp_integration.rs: MCP tool indexing, refresh, alias resolution, circuit breaker integration.
tools/registry/state.rs: all shared mutable state grouped into one ToolRegistryState with sub-structs.
tools/registry/telemetry.rs: event emission, counters.
tools/registry/cache.rs: cached_available_tools, hot LRU cache logic.
tools/registry/policy_facade.rs: ToolPolicyGateway wrappers and catalog syncing.

B) Shared abstractions

Define a single ToolExecutionContext used everywhere:
workspace root
plan mode state
harness metadata snapshot
timeouts policy
policy decision
progress callback
Define a single ToolExecutorPipeline trait used by:
registry execution
agent runner (so runner calls “one thing”)

C) Agent loop alignment

Registry should produce a canonical ToolExecutionRecord that runner/tool_outcomes can consume without re-deriving display formatting or error classification.

D) Tool execution consolidation

Consolidate:
“normalize tool output”
“summarize tool output”
“spool large output”
“record history + telemetry”
into one post-processing step invoked by all executors.

E) State management improvements

Current state uses many independent locks (RwLock, Mutex, atomics). Simplify:
Group into struct ToolRegistryState { mcp: McpState, pty: PtyState, cache: CacheState, counters: Counters, … }
Prefer tokio locks in async flows; avoid holding std locks across .await.
Keep atomics for hot counters only; everything else behind one lock per subsystem.

F) Error handling standardization

Ensure all tool failures become ToolExecutionError with:
ToolErrorType classification
retryable
user_message vs debug_message
Avoid returning anyhow from deep layers; convert at module boundary.

---

PRIORITY 2 (High)

4) vtcode-core/src/config/models.rs (2577)

A) Logical boundaries

config/models/catalog.rs: static model/provider definitions.
config/models/selection.rs: model resolution rules (defaults, overrides, env).
config/models/capabilities.rs: structured output support, tools support, max tokens, reasoning effort.
config/models/validation.rs: validate config + helpful errors.
config/models/serde.rs: config structs and parsing glue.
config/models/tests.rs: resolution + compatibility tests.

B) Shared abstractions

ModelSpec, ProviderSpec, Capabilities reused by providers and runner (to avoid duplicating “supports X” logic).

C) Agent loop alignment

Runner should not contain provider/model conditional logic; it should consult Capabilities.

D) Tool execution consolidation

Tool parallelization safety could be capability-driven (e.g., “model supports parallel tool calls well” is likely not needed; keep it tool-driven, but ensure config isn’t leaking into execution).

E) State management

Make model config immutable snapshots; avoid recalculating/reading env repeatedly.

F) Error handling

Create typed config errors: InvalidModel, UnsupportedCapability, UnknownProvider.

---

5) vtcode-core/src/mcp/mod.rs (2505)

The module already has submodules, but the top-level file still owns too much orchestration.

A) Logical boundaries

mcp/client.rs: McpClient struct + public API.
mcp/init.rs: provider connect + initialize flows (sequential/parallel if ever added).
mcp/index.rs: tool/resource/prompt provider index maintenance.
mcp/validation.rs: validate_tool_arguments, schema validation toggles.
mcp/collect.rs: collect_tools/resources/prompts (including refresh behavior).
Keep existing errors, schema, tool_discovery_cache, connection_pool as-is.

B) Shared abstractions

Shared TimeoutPolicy / RequestBudget concept: unify how MCP and ToolRegistry pick timeouts and retries.
Shared circuit breaker primitives (ToolRegistry already has one for MCP—avoid duplicating “failure tracker” logic).

C) Agent loop alignment

Agent runner should not perform MCP-specific logic; ToolRegistry should be the single tool surface. MCP stays “behind” ToolRegistry.

D) Tool execution consolidation

MCP tool invocation should implement the same ToolExecutorPipeline interface (preflight/postprocess identical).

E) State management

Consolidate indices + providers under one lock domain to avoid inconsistent snapshots:
e.g., McpState { providers, allowlist, indices }
Avoid cloning provider lists repeatedly; provide iterator helpers that hold read locks briefly.

F) Error handling

Convert MCP errors into a shared tool-layer error classification:
tool_not_found, provider_unavailable, schema_invalid, invocation_failed
with retryable where relevant.

---

6) src/acp/zed.rs (2505)

A) Logical boundaries

acp/zed/protocol.rs: message types, serialization, constants.
acp/zed/client.rs: connection + request/response handling.
acp/zed/events.rs: event mapping into agent events.
acp/zed/errors.rs: typed errors.
acp/zed/tests.rs: protocol fixture tests.

B) Shared abstractions

Reuse a common “editor integration transport” trait if VSCode/others exist (don’t invent if not needed; keep minimal).

C) Agent loop alignment

Standardize how “external events” are fed to the same event sink mechanism runner uses.

D) Tool execution consolidation

If Zed triggers tool calls, route through ToolRegistry only.

E) State management

Separate connection lifecycle state from message handling logic; avoid sharing mutable state across unrelated responsibilities.

F) Error handling

Typed protocol errors with context (frame decoding, invalid payload, disconnected).

---

7) vtcode-core/src/tools/file_ops/tool.rs (2503)

A) Logical boundaries

file_ops/ops/read.rs, write.rs, edit.rs, create.rs, list.rs.
file_ops/path_policy.rs: workspace-root enforcement, traversal prevention.
file_ops/diff.rs: patch generation/validation (if present).
file_ops/format.rs: output shaping for LLM consumption.
file_ops/errors.rs: typed errors (permission, not_found, too_large, invalid_encoding).
file_ops/tests/…: filesystem fixtures.

B) Shared abstractions

WorkspaceFs trait (real + test impl) to test without touching disk.
Shared “large output spooling” utility should be called by all tools, not only file ops.

C) Agent loop alignment

Runner’s “last_file_path/last_dir_path” heuristics should align with the tool schema (consistent arg names like path vs file_path).

D) Tool execution consolidation

File ops should not do policy checks itself (except path safety); rely on registry preflight.

E) State management

Keep no mutable state inside the tool; everything derived from args + workspace snapshot.

F) Error handling

Map file errors into ToolErrorType consistently (user-fixable vs internal).

---

8) vtcode-core/src/core/agent/runner.rs (2448)

This currently mixes: conversation building, streaming, tool dispatch, loop detection, and state transitions.

A) Logical boundaries

agent/runner/mod.rs: small facade + AgentRunner struct.
agent/runner/turn_loop.rs: per-turn orchestration (ask model → handle response → maybe run tools).
agent/runner/tool_dispatch.rs: tool-call normalization, parallelization decisions, integration with ToolRegistry pipeline.
agent/runner/streaming.rs: streaming request + fallbacks/cooldowns.
agent/runner/state_updates.rs: TaskRunState mutations (last paths, warnings, completion).
agent/runner/telemetry.rs: event recording glue.

B) Shared abstractions

TurnContext struct: everything needed to run one turn without reaching into many fields.
Reuse the same ToolExecutorPipeline as ToolRegistry, so runner doesn’t duplicate execution bookkeeping.

C) Agent loop alignment

Align with src/agent/runloop/unified/... by choosing one canonical loop:
Either migrate runner logic into unified runloop, or make unified runloop call into runner modules.
Biggest win: eliminate duplicate “tool outcome” shaping and loop detection logic.

D) Tool execution consolidation

Remove runner’s direct calls like tool_registry.execute_tool_ref scattered inside; replace with tool_registry.execute(call, ctx) returning a canonical outcome struct.

E) State management improvements

Reduce RefCell usage by making “mutable per-run” state explicit in a RunState struct passed down the call stack.
Avoid holding locks or cloning large config repeatedly; store immutable snapshots.

F) Error handling standardization

Define AgentTurnError typed enum:
provider errors (retryable)
tool errors (classified)
internal invariants
Keep anyhow only at the CLI boundary.

---

9) vtcode-core/src/llm/providers/openrouter.rs (2331)

A) Logical boundaries

openrouter/provider.rs: public provider impl.
openrouter/request_builder.rs
openrouter/response_parser.rs
openrouter/stream_decoder.rs
openrouter/model_mapping.rs: OpenRouter model ids → internal ids/capabilities (if present).
openrouter/error_mapping.rs

B) Shared abstractions

Same provider shared stack as Anthropic/OpenAI.
Shared “base URL override + headers + API key resolution” helper.

C) Agent loop alignment

Ensure tool call formatting matches OpenAI style if OpenRouter returns OpenAI-like payloads—avoid runner conditionals.

D) Tool execution consolidation

Tool schema should be shared; OpenRouter shouldn’t re-declare tool JSON schema rules.

E) State management

Immutable provider instance; per-request opts passed in.

F) Error handling

Normalize “provider routing errors” into LLMErrorKind::UpstreamUnavailable with retry hints.

---

10) vtcode-core/src/tools/pty.rs (2103)

A) Logical boundaries

pty/manager.rs: PtyManager high-level API.
pty/session.rs: session lifecycle + guard types.
pty/process.rs: spawn/terminate child processes.
pty/poll.rs: polling loop, throttling, output buffering.
pty/platform/{unix,windows}.rs if needed.
pty/errors.rs

B) Shared abstractions

Shared rate limiting / backpressure primitives (also relevant for streaming tool outputs).

C) Agent loop alignment

Ensure PTY tool outputs stream through the same progress callback and spooler pipeline as other tools.

D) Tool execution consolidation

PTY-backed tools should still go through registry execution pipeline for telemetry/history/timeouts.

E) State management

Make counters/active session tracking a dedicated PtyState.
Avoid mixing “policy” (can start session?) with “mechanics” (spawn) in the same module.

F) Error handling

Typed PTY errors: spawn failed, session limit, IO, timeout, terminated.

---

11) src/agent/runloop/unified/turn/tool_outcomes.rs (2007)

A) Logical boundaries

runloop/unified/turn/tool_outcomes/types.rs: outcome structs/enums.
runloop/unified/turn/tool_outcomes/format.rs: formatting for model messages vs UI.
runloop/unified/turn/tool_outcomes/apply.rs: apply outcomes to conversation/task state.
runloop/unified/turn/tool_outcomes/errors.rs: mapping tool errors to outcomes.

B) Shared abstractions

A single canonical ToolOutcome type used by both:
core/agent/runner.rs
unified runloop
tool registry history/telemetry

C) Agent loop alignment

Pick one representation for:
tool success content
tool error content
“tool denied” content
“tool requested clarification” content
and ensure both loops use it.

D) Tool execution consolidation

Outcome creation should happen right after tool execution in ToolRegistry pipeline, not re-derived later.

E) State management

Make outcome application pure: fn apply(outcome, &mut TaskRunState, &mut Conversation).

F) Error handling

Use ToolExecutionError + classify_error as the single classification input; no string matching.

---

4) Cross-cutting “shared abstractions” (the high-leverage reuse)

Implement minimally (don’t over-engineer), but consistently:

Provider shared stack (vtcode-core/src/llm/providers/shared/…)
request.rs: shared header building, base URL normalization.
streaming.rs: generic SSE framing + per-provider delta decoding hook.
errors.rs: ProviderErrorMapper + normalized LLMErrorKind.
tools.rs: tool schema normalization (function name/args/id consistency).
Tool execution pipeline (vtcode-core/src/tools/execution/…)
preflight: policy/loop/rate-limit/timeout decision
execute: dispatch to builtin/MCP/PTY
postprocess: normalize/summarize/spool/history/telemetry
returns ToolOutcome (shared)

This is the main separation-of-concerns win: runner becomes orchestration only, registry becomes tool platform, providers become transport adapters.

---

5) Risks and guardrails

Risk: behavioral drift during extraction (especially streaming + tool call JSON).
Guardrail: golden tests + fixture replay for each provider, plus snapshot tests for tool outcomes.
Risk: lock usage across .await in ToolRegistry/MCP causing deadlocks.
Guardrail: enforce “no std::sync lock held across await” with targeted code review and small helper scopes.
Risk: duplicated agent loops (runner vs unified runloop) diverge further.
Guardrail: converge on one canonical ToolOutcome + one tool execution pipeline immediately (Phase 1).

---

6) When to consider the advanced path

Only if you hit these triggers:

Provider implementations still diverge after decomposition (lots of if/else in runner).
Streaming bugs recur across providers.
Tool execution policy/telemetry inconsistencies persist.
Significant perf issues remain from lock contention or repeated cloning.

Optional advanced path (brief)

Introduce a provider plugin interface with compile-time feature flags and a shared “middleware” chain (retry → tracing → caching).
Introduce a formal state machine for turns (explicit transitions) if loop logic remains complex.

---

If you apply only one thing first: extract the tool execution pipeline + ToolOutcome type and make both runner and unified runloop use it. That yields immediate modularity improvements and reduces reliability bugs caused by duplicated tool bookkeeping.