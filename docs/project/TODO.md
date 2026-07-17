---
add license https://github.com/xai-org/grok-build#license

some tools we use from codex
---

reference /Users/vinhnguyenxuan/Developer/learn-by-doing/grok-build

1. project structure
2. rust config
3. cargo.toml
4. fmt
5. clippy

===

# Token-overhead sweep — status

## Goal

Drive the default, out-of-the-box first-request token overhead (system prompt + tool schemas + instructions) below 12k tokens, and keep per-turn overhead growth bounded, while preserving capability. Targets the same mechanisms the original article blames: oversized tool schemas, unstable cache prefixes, heavy subagent bootstraps, and redundant history.

## What is already implemented (verified)

| Area               | Mechanism                                                                                                                                                                                                       | Location                                                       | Guard test                                                                                            |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Tool schema tax    | `DIRECT_TOOL_EXPOSURE_THRESHOLD = 15` (matches builtin cap); MCP tools defer whenever present (any count)                                                                                                       | `vtcode-core/src/tools/handlers/session_tool_catalog.rs`       | `mcp_deferral_keeps_first_request_wire_payload_near_baseline`                                         |
| Tool schema tax    | Token-budget backstop `DIRECT_TOOL_EXPOSURE_TOKEN_BUDGET = 4_000` forces deferral even under threshold                                                                                                          | `session_tool_catalog.rs`                                      | same                                                                                                  |
| Tool schema tax    | `client_tool_search` defaults to `true` (client-local deferral for providers without hosted tool search)                                                                                                        | `vtcode-config/src/core/tools.rs`                              | `client_tool_search_defaults_to_enabled`                                                              |
| Tool schema tax    | MCP description cap `MCP_TOOL_DESCRIPTION_MAX_LEN = 512`                                                                                                                                                        | `session_tool_catalog.rs`                                      | —                                                                                                     |
| Tool schema tax    | Progressive-mode builtin schema ≤ 3k tokens, builtin count ≤ 14                                                                                                                                                 | `vtcode-core/src/tools/registry/builtins.rs`                   | `emitted_model_tool_schema_fits_within_first_request_budget`                                          |
| Cache stability    | `cache_friendly_prompt_shaping` moves volatile runtime context to the trailing section; stable-prefix hash                                                                                                      | `turn_processing/llm_request/snapshot.rs`, `hash_utils.rs`     | `stable_prefix_hash_ignores_runtime_*`                                                                |
| Subagent bootstrap | Lightweight default child profile: `system_prompt_mode=minimal`, `tool_documentation_mode=minimal`, no inherited MCP servers unless explicitly requested                                                        | `vtcode-core/src/subagents/config.rs`                          | `default_subagent_bootstrap_tokens_are_materially_below_parent` (≤ 80% of parent, NOT 40% — see note) |
| History growth     | `tool_result_clearing.enabled` defaults to `true`                                                                                                                                                               | `vtcode-config/src/core/agent.rs`                              | —                                                                                                     |
| Lazy capabilities  | MCP is on-demand: `AsyncMcpManager` is created at session boot in `Initializing` state but the connect task is only kicked off by `/mcp` activation / slash command / reconfigure — no eager connect at startup | `session_setup/init.rs`, `async_mcp_manager.rs`                | —                                                                                                     |
| Telemetry          | Per-request `token_budget_breakdown` metric (system-prompt / tool-schema / message-history tokens + on-wire tool count) emitted to `vtcode.turn.metrics` + trajectory log from the real assembled wire request  | `turn_processing/llm_request/metrics.rs`, `request_builder.rs` | —                                                                                                     |

Cache read/write/miss counts are NOT duplicated in the new metric — they already live in `SessionStats` prompt-cache diagnostics. `instruction_file_tokens` is not separated out because instruction-file content is merged into the final system prompt during assembly; it is included in `system_prompt_tokens`. `subagent_bootstrap_tokens` is a spawn-time concern tracked separately, not per-request.

## Gaps closed in this session

1. **MCP deferral payload regression test** — `mcp_deferral_keeps_first_request_wire_payload_near_baseline` builds five simulated MCP server tools, compares eager vs client-local deferred wire payload, and verifies the deferred first-request payload stays within 25% of the no-MCP baseline. (`session_tool_catalog.rs`)
2. **Subagent bootstrap token-count regression test** — `default_subagent_bootstrap_tokens_are_materially_below_parent` composes real parent Default and child Minimal system prompts under deterministic settings and asserts the child is ≤ 80% of the parent. (`subagents/config.rs`)
3. **Unified per-request token-breakdown telemetry** — `emit_token_budget_breakdown` / `TokenBudgetBreakdown` records the assembled wire-request prefix breakdown. (`metrics.rs` + `request_builder.rs`)

### Note on the 80% threshold

The originally-speculated ≤ 40% child-prompt target is false. Measured values are ~888 parent / ~665 child tokens (~75%). The Minimal-vs-Default base-contract difference is bounded; the larger subagent bootstrap savings come from dropping the inherited MCP/tool catalog, not the prompt mode alone. The guard asserts ≤ 80% so a regression that bloats the Minimal profile is caught without encoding a false target.

## Genuinely remaining work

- **Config validation warnings (Phase 7.2)** — warn at startup when MCP tool count exceeds the deferred threshold, estimated tool-schema tokens exceed the budget, or the system prompt exceeds `max_system_prompt_tokens`. Not yet implemented.
- **User-facing docs (Phase 8.1)** — `docs/config/CONFIG_FIELD_REFERENCE.md` and `docs/tools/TOOL_SEARCH.md` should record the current deferral defaults + the new telemetry target; a short "how to audit first-request token cost" note for users. Module `AGENTS.md` for `vtcode-core` and the binary (`src/AGENTS.md`) are already updated.
- **History-growth tuning verify items (Phase 5.2–5.4)** — auto-compaction threshold aggressiveness, reasoning/thinking block stripping during compaction, and repeated tool-call loop caps. These are separate history-growth concerns, not first-request overhead; verify current behavior before changing.

## Verification performed

- `cargo nextest run -p vtcode-core -E 'test(mcp_deferral_keeps_first_request_wire_payload_near_baseline) | test(default_subagent_bootstrap_tokens_are_materially_below_parent)'` — pass.
- `cargo check --bin vtcode`, `cargo clippy -p vtcode-core --tests -- -D warnings`, `cargo clippy --bin vtcode --tests -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean.

### Known unrelated pre-existing failure (NOT caused by this work)

`agent::runloop::unified::turn::compaction::tests::recovery_context_previews_extract_structured_tool_guidance` fails on a clean tree (verified via `git stash` → run → `git stash pop`): the preview does not contain the expected `"No matches found in src/agent"` substring. Do not fix it as part of this token-overhead sweep.

===

add license https://github.com/xai-org/grok-build#license

some tools we use from codex

---

reference /Users/vinhnguyenxuan/Developer/learn-by-doing/grok-build

1. project structure
2. rust config
3. cargo.toml
4. fmt
5. clippy

===
