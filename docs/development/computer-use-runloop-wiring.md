# Computer-use run-loop wiring

How the committed computer-use foundation (`vtcode-computer-use`, the
OpenAI/Anthropic codecs, the isolated browser target) connects to the
unified run loop. The approved plan (`Native computer use in VT Code`)
owns the product requirements; this doc pins the integration seams in
the current tree.

## Authority split (recap)

Providers describe actions. The run loop owns turn identity, approval,
ordering, and cancellation. Drivers own target verification and input
delivery. Nothing below moves those lines.

## Inbound: provider response to native calls

`LLMResponse.tool_calls` (`crates/common/vtcode-commons/src/llm/mod.rs`)
is the only model-to-loop tool channel today. Native items must cross it
without entering function dispatch:

- Keep the fail-closed guards as the default. Bound turns call the
  capability-based `LLMProvider::generate_native` (default: unsupported,
  no I/O) instead of `generate`: each provider checks the verified gate
  first, declares its native tool, and parses native items into typed
  calls instead of returning `unbound_route_error`. No shared
  `LLMResponse`/`LLMRequest` fields change. Streaming needs the same
  treatment later (`stream_native` plus native accumulation in the
  Anthropic and Responses stream decoders).
- Preserve mixed-turn positions. Every `NativeCall` carries its content
  position in `NativeCall::index`, and the Anthropic adapter returns
  `Vec<(usize, Value)>` so the loop can merge ordinary results at the
  intervening slots; the OpenAI adapter consumes one batched call.
- Decode with the committed `decode_call` codecs. A decode error on a
  bound route revokes the target lease; it must never fall through to
  function dispatch or silent skip.

## Approval

Reuse the tool permission flow, not a parallel prompt system:

- `build_tool_permissions_context_with_safety`
  (`src/agent/runloop/unified/turn/tool_outcomes/handlers/mod.rs`) and
  the `approval_recorder` decide per-call permission today.
- Emit `permission.requested` / `permission.resolved` (`ThreadEvent`)
  with a `computer_use` tool identity. The prompt must name the target
  (build directory or app) and the screenshot recipient, per the plan.
- Approval is per turn and per target. Denial, or any new control
  sequence after resume, starts over: no lease reuse, no replay.
- Computer-use approval must resolve through the human reviewer even
  when automation policies would auto-approve ordinary tools (the
  Codex `NeedsHumanApproval` analog): no caching, no remembered
  targets, no full-auto bypass in v1.
- Only after approval construct `TurnControl::after_approval` with the
  active turn's `CancellationToken`. The lease binds the turn, not the
  issuing agent: subagent completion does not revoke it, and the turn
  runner owns the handle, never a subagent scope.
- When `computer_use.decisions_review` is set, each proposed batch is
  screened by OpenAI Decisions (task text plus action kinds/counts,
  never screenshots or typed values) before execution: `SUSPECT` halts
  the turn, while review outages degrade to proceed-with-warning
  because the human approval remains the primary control. OpenAI
  routes only.

## Execution

The turn-scoped owner is `run_computer_turn`
(`src/agent/runloop/unified/turn/tool_outcomes/handlers/computer_use/execution.rs`):
generate over the bound route, budget check, Decisions review, injected
human approval, then launch, execute, and tear down through one funnel.
Cancellation and timeouts take the same teardown as completion and
failure, and a teardown failure after completion flips the verdict.

- Launch via `browser::container` (validated spec, pinned image,
  argv-locked hardening flags) and attach `BrowserDriver` to the pipes.
- Execute decoded natives through the provider adapters
  (`openai::computer_use::execute_decoded_call`,
  `anthropic::computer_use::execute_decoded`) against the `TurnControl`.
  The raw-block adapters (`execute_call`, `execute_batch`) stay for the
  unbound/codec paths; the dispatcher never re-decodes wire.
- The Stop path funnels through the dispatcher's `wait_for_stop` select
  and post-launch check into `ContainerHandle::terminate`, so delivery
  interrupted mid-action still removes the exact container. The runner
  also exits on its own after any failed action or input close; treat
  that as backstop, not the primary path.
- Every teardown records its trigger (finished, failed, stopped) with
  the `TerminateOutcome` in the redacted turn audit, mirroring the
  `turn_ended` hook contract: idempotent, trigger-named, turn-keyed.

## Outbound: results to the next request

`prepare_tool_response_content`
(`src/agent/runloop/unified/turn/tool_outcomes/response_content.rs`)
flattens ordinary results to `String`. Native results bypass it:

- OpenAI: append the original `replay_item()` plus the produced
  `computer_call_output` to the next Responses `input`, preserving
  `store=false` stateless replay. Prove continuation per route.
- Anthropic: emit ordered `tool_result` blocks at their original
  positions, merged with ordinary client-tool results.
- Request builders (`anthropic/request_builder/messages.rs`,
  `openai/request_builder/responses.rs`) need a typed native-results
  input; a `Message` text field cannot carry screenshots.

## Redaction, events, and resume

- `ThreadEvent` tool items (`ToolInvocationItem.arguments`,
  `ToolOutputItem`) carry summaries only: action kinds and counts, no
  typed text, no base64. Add a redacted-summary builder plus a sentinel
  scan over durable outputs and traces.
- Session persistence must exclude screenshot bytes and typed values.
- Resume rebuilds valid provider history from summaries and starts any
  new control sequence with fresh approval and observation. Open
  question: the exact resumed-history shape per provider (redacted
  placeholder blocks vs. truncated turns). Decide with provider
  contract tests before shipping resume.

## Verified gate

Declare the `computer` / `computer_toolset_*` tools only when the
triple (model, endpoint, protocol version) plus a usable target passed
the acceptance runs in the plan. Ship the gate as a pure predicate
with the verified matrix beside it; an empty matrix denies everything.
Wire the gate into tool declaration, not into decode: undeclared tools
must never produce native items.

## Config

New `computer_use` section in `vtcode-config`: enabled flag, target
(browser build directory; macOS app identity later), image pin, and
per-turn timeout. Document each field where the config reference lives.

## Activation surfaces (landed)

Both apps share the `computer_use` config section with `enabled` as the
master switch; per-turn human approval still gates every turn, and v1
has no always-allowed list. CLI: `/computer-use [on|off|status|target
<path>]` (alias `/cua`) plus a Computer Use group in `/config` (the
`browser_build` target is set via the command because None-default
free-form paths do not render in curated groups). Desktop: a Computer
Use settings tab (master Switch, Decisions Switch, target summary,
deep link into CLI configuration), a ⌘K "Computer use: Turn on/off"
entry, and `settings/computer-use` + `action/toggle-computer-use`
deeplinks. No composer button: the composer rows hold a pixel-contract
single line and a fifth labeled button wraps the zen row.

## Turn trigger (landed: explicit user invocation)

`/computer-use run <task>` runs one turn end to end: it builds the
request from the session model, calls `run_computer_turn`, confirms the
proposed batch through an inline overlay (Esc denies; every
non-confirm outcome denies), records usage, and renders a redacted
report. The overlay replaces the permission flow for user-initiated
turns only: the typed command already carries human intent, and the
overlay has no cache or auto-approval surface at all. Model-initiated
turns (a future `computer` tool) must approve through
`request_target_approval` instead, since only the permission flow
defends the confused deputy. The dispatcher re-checks enabled, target,
and provider support; the command pre-checks the same gates with
usage guidance before spending a model call.

## Then

Model-initiated invocation (tool registration, schema, prompt
guidance), multi-turn chaining/continuation, streaming native
accumulation, and the verified model/action matrix. After that: prompt guidance
(`vtcode-core/src/prompts/guidelines/`), tool schema
(`vtcode-utility-tool-specs`), `ThreadEvent` contract updates, budget /
presence tests, user guide plus quick reference, and the verified
model/action matrix — per the repo's major-feature docs rule. macOS
driver isolation and the Desktop Settings pane stay a separate release
gate behind signed-app acceptance.
