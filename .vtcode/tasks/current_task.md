# Explore core agent loop

- [x] Identify relevant files/docs for agent loop
  files: docs/project/project_analysis.md, docs/guides/async-architecture.md, src/agent/runloop/unified/mod.rs, src/agent/runloop/unified/turn/session_loop_impl.rs, src/agent/runloop/unified/turn/turn_loop.rs
  outcome: Core loop spans `session_runtime.rs` → `turn/session_loop_impl.rs` + `turn/run_loop_context.rs`; per-turn flow orchestrated by `turn_loop.rs` with context pruning, model selection, tool routing, UI streaming, approvals, and finalization modules.
- [x] Summarize loop structure and flow
  files: src/agent/runloop/unified/turn/session_loop_impl.rs, src/agent/runloop/unified/turn/turn_loop.rs, src/agent/runloop/unified/context_manager.rs, src/agent/runloop/unified/tool_pipeline.rs
  outcome: Unified session runtime initializes session state, then `turn/session_loop_impl.rs` drives turn iterations via `turn_loop.rs`. Each turn hydrates context, runs LLM planning, handles tool calls through `tool_pipeline.rs`, streams UI via `ui_interaction.rs`, and finalizes with approvals and notifications. Context pruning/compaction handled by `context_manager.rs`; tool safety enforced by `tool_call_safety.rs` with circuit breakers and rate limiters.
- [x] Note key entry points for further work
  files: src/agent/runloop/unified/session_runtime.rs, src/agent/runloop/unified/turn/session_loop_impl.rs, src/agent/runloop/unified/turn/turn_loop.rs, src/agent/runloop/unified/run_loop_context.rs
  outcome: Primary entry points: `UnifiedSessionRuntime::run_session` wires CLI sessions into `turn::run_single_agent_loop_unified`. Within turn execution, `session_loop_impl.rs` sets up `SessionState`, telemetry, and `HarnessTurnState`, kicking off `turn_loop::run_turn_loop`. Tool execution and approvals route through `turn::tool_handling` + `tool_pipeline.rs`, while UI streaming flows through `ui_interaction.rs`. `run_loop_context.rs` exposes shared state for instrumentation, making these files the main hooks for enhancements.
