# vtcode Binary Gotchas

This guide preserves detailed maintainer notes for the binary crate. Keep `src/AGENTS.md` concise; use this page when
changing the runloop, update path, allocator, or request assembly.

## Startup, updates, and allocation

- TUI shutdown ownership: the spawned TUI task (`spawn_session_with_options`) owns terminal restoration via
  `panic_hook::restore_tui()`. `finalize_session` must await `InlineSession::wait_for_exit()` after `handle.shutdown()`
  before calling `restore_terminal_on_exit()` as a backstop — `restore_tui()` is idempotent through the global
  `RESTORE_DONE` flag, so whoever claims it first wins, and a host restore issued on a fixed sleep races the TUI's final
  frames onto the main screen (transcript leaks into CLI scrollback). The TUI side mirrors this with
  `is_restore_claimed()` guards and the shared terminal-operation lock: `drive_terminal` exits without rendering,
  `render_if_dirty` rechecks the claim while holding the lock, and `run_tui` skips `finalize_terminal` once restore is
  claimed.
- Standalone updates own asset selection, checksum verification, safe archive extraction, and `self_replace`; keep
  managed-install guidance separate and keep inline TUI downloads quiet.
- `main_helpers` handles runtime relaunch context; do not duplicate initialization logic in `main.rs`.
- Allocator memory pinning: vtcode's bursty/sparse Tokio workload (semaphore-capped `JoinSet` fan-out followed by idle
  workers) can leave both `mimalloc` and `glibc` RSS flat after a burst because frees are stranded on cross-thread lists
  until later allocation activity. `jemalloc` returns memory while idle only when `background_thread` is active; on
  macOS its build reports `background_thread currently supports pthread only` and behaves like mimalloc. On Linux
  containers it can reclaim memory. Measure with `vtcode bench-allocator` before changing the default.
- `load_dotenv()` must run before config load so `.env` API keys are available.
- Debug binaries are hundreds of megabytes: the *first* exec of a fresh `target/debug/vtcode` in a process pays a
  multi-second pre-`main` startup cost (loader work on cold pages), while later execs in that process take
  milliseconds. Tests that race a wall-clock deadline against fixture readiness (for example
  `cli::companion::tests::process_cleanup`) must exec the shipped binary once to warm it, otherwise the deadline
  closes the fixture's owned command before it can publish a pid and the test fails with a bare timeout.
- Provider noise (for example `]<]minimax[>[`) is stripped centrally in `turn::provider_noise`; stream-level harmony and
  MiniMax sanitization lives in `stream_sanitization::StreamSanitizer`. Do not re-implement it inline.

## Tool-budget and recovery contracts

- Wall-clock and tool-call budget exhaustion share one contract: emit the full policy message once, compact stubs for
  later batch calls, then one synthesis directive via `flush_budget_synthesis_directives` after the tool batch. Do not
  break the turn as `Blocked`, because that skips synthesis. The flush arms `switch_to_tool_free_recovery()` so the next
  request removes tool definitions at the API level. User-approved session-limit grants are a separate same-turn retry
  path: emit `SessionToolLimitIncreased`, preserve the granted fuse through `set_limits`/`start_turn`, and keep the
  active primary agent; Copilot appends the shared continuation directive to its ACP tool result because it has no
  normal turn-processing context. Loop-limit grants emit `ToolLoopLimitIncreased` and leave tools enabled. Exclude
  explicit `request_user_input` think-time and command-session `wait` time from the wall-clock calculation; neither
  consumes autonomous execution budget.
- Verification gates successful completion, not edits or inspections. The registry owns required checks,
  mutation generations, owned running sessions, and terminal verdicts. Run only missing checks at completion;
  required checks bypass inspection-result replay. A failed attempt permits at most one corrective pass per genuine
  request, and tracker progress or internal continuations do not replenish it. Existing settings can disable recovery.
  Compare failed generated assertions against the original request and observed artifact before changing code;
  explicitly correct erroneous tracker verification metadata without weakening user criteria, then execute the check.
  Budget exhaustion remains a budget handoff with changes unverified and earlier failures labeled as previous evidence;
  it cannot schedule another verification recovery turn. See [completion verification](completion-verification.md)
  for shell classification, freshness, mandatory tracker checks, and recovery boundaries. Copilot/direct and batch
  execution persist `SessionStats::verification_snapshot` after updating their `LoopTracker`, so resumed turns
  reconstruct the same gate state.
- If the ordinary tool-loop allowance is reached and no manual extension is granted, schedule one tool-free synthesis
  request before ending the turn. The loop allowance becomes unlimited only for that recovery control pass; the provider
  request still has tools disabled. The absolute hard cap and its terminal behavior remain unchanged. After the first
  successful interactive tool-loop grant in a process session, later limit hits auto-grant the maximum remaining `+N`
  without another HITL prompt (`SessionStats::tool_loop_grant_preauthorized`); denials do not latch.
- `tool_budget_exhausted_emitted` participates in the plan-mode `mark_recovery_exhausted` gates. The preflight circuit
  breaker in `turn/tool_outcomes/handlers/mod.rs` follows the same `Continue` plus deferred synthesis path via
  `preflight_circuit_recovery_pending` and `flush_preflight_circuit_recovery`; never return `Break(Blocked)` for an
  approved-plan build.
- Approved-plan implementation turns receive one internal `+50` loop allowance at initialization, clamped by the
  ordinary hard cap; `0` remains unlimited and manual extensions/tool-call budgets stay independent.
- Queue model-facing auto-permission probe warnings while a tool batch runs. Flush them after all tool results and
  before recovery directives; keep the UI warning immediate and deduplicated.
- During tool-free recovery, do not inject `request_user_input`; it violates the recovery contract. Plan-aware fallback
  must preserve interview state, while exhausted budget or recovery caps conclude with the user-facing planning notice
  instead of re-forcing research or emitting a final directive with no following model call.
- In planning, two consecutive empty model responses schedule exactly one tool-free synthesis from the latest request
  and bounded evidence. Require one validated canonical `<proposed_plan>` block; empty, malformed, tool-bearing, or
  unpersistable synthesis emits `turn.failed` with an actionable resumable handoff and keeps planning active.
- Canonical event finalization is independent of optional Open Responses, ATIF, legacy, and WebMCP exporters: a blocked
  turn emits `turn.failed`, while `thread.completed` is reserved for final shutdown. Exporter failures are diagnostics
  and must not prevent the canonical event path from closing.
- The assistant text-response cap applies to consecutive text-only responses, not the cumulative number emitted in a
  turn. The authoritative turn-state streak survives compaction and Copilot inline execution, which is not represented
  in ordinary message history. Reset it only after a tool passes admission; blocked, preflight-invalid, and
  verification-gated attempts are not progress and retain the streak while their dedicated safeguards bound those
  failure loops. Copilot must also record out-of-band tool progress so a failed post-tool synthesis receives the same
  bounded recovery as history-backed tool execution. Reaching the cap is a blocked safety stop, not successful
  completion. If only substantive commentary exists at the cap, promote the latest commentary to the final history phase
  and reuse the existing renderer/stream flags so the user and canonical `ThreadEvent` stream receive exactly one
  preserved response; use the deterministic fallback only when no substantive response exists.
- The interview-denied recovery fallback must distinguish "no draft produced" from "draft persisted."
  `PLANNING_RECOVERY_SYNTHESIS_FALLBACK_NO_INTERVIEW` promises "Review the plan below" and offers
  `yes`/`implement`/`no`/`edit` — those choices dead-end without a persisted plan and contradict the appended
  `PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT`. When `persisted_plan_ready` is false, use
  `PLANNING_INTERVIEW_DENIED_NO_DRAFT_NOTICE` instead (turn_902).
- Permanent `request_user_input` denial is distinct from user cancellation. Mark denial from both execution failure and
  permission-flow denial, suppress the tool for the rest of the session, and route text replies through the shared
  planning intent classifier.

## WebMCP bridge boundary

- WebMCP stays opt-in with explicit origins, loopback defaults, in-memory credentials, terminal/full-auto authorization,
  and the existing full-auto workspace-trust gate before headless mutations/checks. Active bridge replacement and unpair
  require terminal confirmation.
- Bridge prompts use a prompt-only inline event and must not enter slash-command parsing. Keep browser authority
  separate from terminal-owned origins, roots, pairing, and policy.
- The active handoff prompt keeps its `<webmcp_authoritative_diff>` tags and ```` ```diff ```` fence for the model.
  `unified::webmcp::display_turn_handoff` is display-only shaping for the transcript: it splits on those exact markers
  (falling back to the raw prompt when they do not match), hides the structural markers, and styles the untrusted diff
  body through the shared `tool_output::styles` diff palette after ANSI-stripping it. Never re-shape the prompt string
  that reaches conversation history or the provider.

## Request assembly and planning

- `turn_processing/llm_request/` uses contract-carrying `pub(super)` submodules (`snapshot`, `tool_shaping`,
  `context_management`, `response_chain`, `prompt_assembly`, `prompt_sections`, and `prompt_runtime`). Go through
  `mod.rs` exports.
- `ContextManager::normalize_history_for_request` is request-scoped: preserve the borrowed fast path for clean histories
  and use the core normalizer only to repair provider-facing tool-result ordering. Never mutate durable session history
  as part of provider retry.
- Prompt section order in `prompt_assembly::build_prompt_output` is provider-cache-sensitive. Hosted Anthropic/OpenAI
  payloads retain deferred tool definitions; only ClientLocal policy omits them. `metrics.rs` emits the per-request
  `token_budget_breakdown`; prompt-cache diagnostics remain in `SessionStats`.
- Plans stay compact/spec-like: `PLANNING_WORKFLOW_PLAN_QUALITY_LINE` requires roughly 1500 tokens with
  `Action -> files/symbols -> verify:` steps and file/symbol references. The truncation recovery prompt is bounded by
  `MAX_PLAN_SYNTHESIS_CONDENSE_ATTEMPTS`.
- Planning research scales to request complexity; the wall-clock budget is the backstop, not a replacement hard
  tool-count cap. `effective_max_tool_calls_for_turn` and mid-turn config reapplication preserve the shared planning
  floor.
- Approval phrases are classified by `detect_planning_intent` and resolve one typed plan target: ordinary current/fresh
  approval selects Build, while only an explicit Auto choice or configured full-auto policy selects Auto. The target
  carries destination, confirmation policy, context, persisted plan, and tracker through the shared direct/queued
  handoff boundary; it must never restore Auto merely from the source agent. The implementation turn is an explicit
  internal next-turn trigger, not a best-effort steering item; append its synthetic user prompt exactly once after the
  handoff directive so a full steering FIFO cannot leave the selected agent waiting for `continue`.
- Emit `plan.approval.requested` and `plan.approval.resolved` for every decision using the canonical `ThreadEvent`
  contract and preserve Open Responses parity.
- Enter/exit phrase literals live in `vtcode_core::planning` and are shared by the runloop and Codex bridge; add a
  phrase once, never fork consumer-specific aliases.
- Tool-summary renderers receive `ToolSummaryRenderContext` at their public render entry points; pure helpers may remain
  `Option<&Path>`-driven internally.
- Compact tool summaries are session-local; compact mode is the default, contiguous successful command calls share an
  activity row, and bounded live PTY output remains separate from complete captures. Failures, warnings, diffs, stderr,
  and result bodies retain their boundaries. Complete command output is available in Transcript Review through the
  configured primary binding (default `Ctrl+T`), interleaved with the rest of the conversation. Rich review mode is
  default and the configured render binding (default `R`) switches to ANSI-free raw text; the visible review hint, close
  button, and shortcut guide are configurable.
- Ordinary completed turns publish a non-empty final assistant response through both renderer and `ThreadEvent` harness
  paths. The approved-plan handoff is the control-flow exception: its outer loop creates the implementation request, so
  it must remain `Completed { plan_approved_execution_pending: true }` without synthesizing a final response. Recovery
  fallbacks remain visible but produce a blocked outcome; approved-plan summaries retain changed files, verification,
  and blockers. Replacing a session preserves its last turn outcome, and failed plan summaries never depend on fallback
  wording to report failure. Thread budget exhaustion has priority over clean exit or prior completion. Budget rejection
  stubs reuse the shared synthesis note rather than generic retry advice.

For prompt/runtime source boundaries, see [runtime guidance](./runtime-guidance.md).

## Startup benchmark guidance

The standalone startup matrix is deliberately provider-free:

```text
vtcode --version
vtcode --help
vtcode schema tools --format ndjson --name code_search
```

Build and invoke it with the release executable:

```bash
cargo build --release --locked --bin vtcode
VTCODE_BIN="$PWD/target/release/vtcode" \
  VTCODE_BENCH_RUNS=10 cargo bench --locked --bench startup -- --noplot
```

Measure both warm launches and cold fresh-executable-copy launches. A cold sample copies the executable to a new
temporary path for each process; it is a loader/process cold proxy, not an OS page-cache benchmark. The harness does not
flush or evict OS page caches. Isolate each child with temporary `HOME`, config, data, explicit config-file, and
workspace paths so credentials and developer state cannot influence startup.

Results must retain raw samples and report median and p95 per case and mode. Keep the benchmark environment, binary, and
sample count fixed when comparing changes. `VTCODE_STARTUP_TRACE=1` is for a separate diagnostic run only; it emits
bootstrap phase durations to stderr without changing normal CLI output. Metadata commands report `dispatch_ready`, while
the interactive path reports its first rendered frame separately.

## Interactive session readiness

Interactive launches split session bootstrap so first paint does not wait on the full agent runtime:

1. `initialize_session_critical` — provider, one plugin-aware primary-agent discovery pass
   (`discover_controller_subagents`) joined with lightweight `ToolRegistry` (`new_with_loaded_config` reuses the session
   config, no second workspace parse), resume history, cheap `SessionBootstrap` (workspace language/guideline scans
   skipped), seed system prompt, preflight-only update notice (no `Updater::new` disk read, no release-notes read),
   deferred approval recorder (paths only, no `mkdir`, no pattern reads), first-paint `ToolRegistry` (no workspace TOML
   re-parse, no policy file read/create). Discovery result is cached in `SessionState.discovered_subagents`. MCP manager
   is created but background initialization is deferred.
2. `initialize_session_ui` — spawn TUI / first frame with built-in slash commands; prompt templates merge post-paint via
   `SetSlashCommands`. Hook approval prompts may run here; session-start hooks do not.
3. `hydrate_session_runtime` — deferred update-cache + release-notes reads (merged into header highlights),
   approval-cache dir ensure + pattern reload, workspace policy manager attach, tool registry async init + policy +
   CGP + model-tool projection + skill tools, system-prompt composition into `ContextManager`, subagent controller via
   `new_with_discovered` (no second scan), trajectory, dynamic context, MCP reconfigure/restart (first MCP init starts
   here).
4. `apply_post_hydration_ui` + `run_session_start_hooks` — re-drive agent palette/refresh/header/full-auto banner/budget
   warning, then execute session-start hooks against the hydrated tool registry.

Hydration and post-hydration hook execution are awaited before the interaction loop dispatches the first model turn.
Setup failures still abort the session. Registry-light critical path: `initialize_session_critical` must not construct
`ToolRegistry` or call `discover_controller_subagents` (see `critical_path_avoids_registry_and_discovery`). Those run in
`complete_session_registry` after first paint.

Static-first typeable shell: `initialize_session_shell` spawns a typeable TUI before ToolRegistry/discovery/provider
construction. Do not move heavy init back before `spawn_session_with_options` (see shell ratchet test). The one await
allowed there is the bounded palette probe (`await_terminal_palette_probe`, ~50 ms read + 40 ms settle + 100 ms drain
worst case, usually instant after overlapping startup resolution): awaiting before crossterm owns the TTY keeps late
`OSC 10/11/4` replies from winning the `/dev/tty` read race and leaking as `10;rgb:...` keystrokes. Slow-terminal
stragglers are swallowed by the vendored crossterm `parse_osc` backstop (`InternalEvent::OscResponse`, never matched by
`EventFilter`).

Maintenance never sits on first paint: interactive legacy path migration is fire-and-forget `spawn_blocking` before
dispatch, harness session-store retention runs in `run_harness_retention` spawned after `initialize_session_ui`, and the
palette probe overlaps bootstrap so its pre-spawn await rarely waits — `note_crossterm_raw_mode` still guards the
`RawModeGuard` restore, and `await_terminal_palette_probe` after spawn settles theme before the first model turn (probe
timeout 50 ms).

Trace phases: `session_setup_critical`, `session_setup_ui`, `session_setup_hydrate`, `session_setup`, `first_ui_render`.

## Exit and shutdown tail

Fullscreen TUI exit must return to the shell promptly. Every blocking step between `drive_terminal`
returning and the postamble is budgeted so a single stuck child, hook, or blocking task cannot park the
process after the terminal is already restored:

- `finalize_session` bounds session-end hooks (500 ms interrupt / 3 s normal) and MCP shutdown (500 ms / 2 s),
  then `handle.shutdown()` + `InlineSession::wait_for_exit(2 s)` (abort fallback). Keep these bounds.
- The exec/PTY backstop in `orchestration.rs` (`terminate_all_exec_sessions_async`) and the subagent
  controller `signal_shutdown()` are wrapped in 2 s timeouts. Each exec-session close can block up to
  `EXEC_SESSION_CLOSE_TIMEOUT` (12 s) on a stuck child reap and closes run sequentially, so an unbounded
  await here could stall exit for minutes; the OS reaps any remainder at process exit.
- `ProcessHandle::drop` (vtcode-bash-runner) performs synchronous kill + task aborts only. It must never
  bridge through a spawned thread + temporary Tokio runtime — that added spawn latency per handle and
  blocked the dropping thread during registry teardown. Regression:
  `drop_kills_child_synchronously_without_bridging_runtime`.
- `EventStreamController::stop`/`shutdown` abort the crossterm event-loop task when it does not stop within
  100 ms, so a wedged reader cannot keep stdin claimed or delay alternate-screen teardown.
  `SignalCleanupGuard::drop` bounds its join of the SIGTERM handler thread (100 ms spin, then detach).
- `main.rs` calls `runtime.shutdown_timeout(500 ms)` after `block_on(run(...))`. Tokio's blocking pool waits
  indefinitely for in-flight `spawn_blocking` work on plain drop; a stuck git snapshot or child reap would
  otherwise park the process after the summary. Normal teardown finishes well within the budget.

## Blocker forensics and retention pins

Blocked handoffs copy session `events.jsonl` / ATIF into `{archive}-forensics/` beside the blocker archive (best-effort;
missing sources do not fail the handoff) and write `retention-pin.json` under the session directory so ordinary session
retention cannot evict forensics while the blocker is unresolved. Resolving the archive unpins only when no other
unresolved archive still references that session.
