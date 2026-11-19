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
has no always-allowed list. CLI: the Computer Use group in `/config`
(enabled, browser target, image, review, limits — unset free-form
fields render editable rows that create the node on submit). Desktop: a
Computer Use settings tab (master Switch, Decisions Switch, target
summary, user guide, deep link into CLI configuration), a ⌘K
"Computer use: Turn on/off" entry, and
`settings/computer-use` + `action/toggle-computer-use` deeplinks. No
composer button: the composer rows hold a pixel-contract single line
and a fifth labeled button wraps the zen row. There is no
`/computer-use` slash command: the switch and target stay
human-operated; the model only ever runs turns.

## Turn trigger (landed: model tool calls)

Turns start when the model calls the registered `computer_use` tool
with a plain-words `task`. The turn loop intercepts the validated call
and runs the dispatcher: propose (gates, native generation, budget,
Decisions review), approve the proposed batch through the permission
flow (`request_target_approval`), execute, tear down. Single approval
per turn: validation skips the generic scope prompt for `computer_use`
(the batch review shows what would actually execute), while every deny
path (hook, cache, policy, permission, full-auto) and
safety-gateway-flagged prompts still apply, and the batch prompt is
never auto-approvable. Fail-closed refusals (disabled, missing target,
unsupported provider, unverified model) return as tool errors carrying
guidance the model relays. The registry executor for the tool is a
fail-closed stub: execution only ever happens through the turn
dispatcher.

Shell screenshots route here too: while computer use is runnable
(advertised, enabled, target configured, provider capable), an
`exec_command` carrying a headless `--screenshot` flag or an
open-URL-plus-`screencapture` compound is blocked in validation with a
steering error, so the model re-issues the task as `computer_use`.
Bare `open <url>` (viewing) and bare `screencapture` (desktop captures
v1 cannot replace) pass through, as does everything when computer use
cannot run. The master switch stays the human-operated escape hatch
for sessions that need raw headless output.

## Then

Multi-turn chaining/continuation, streaming native accumulation, and
more verified model/action matrix entries. After that: `ThreadEvent`
contract updates if turn-specific events prove necessary, and the user
guide plus quick reference — per the repo's major-feature docs rule.
macOS driver isolation stays a separate release gate behind signed-app
acceptance.
