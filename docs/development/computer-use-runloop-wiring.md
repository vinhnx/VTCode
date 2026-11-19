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
- Preserve mixed-turn positions. The Anthropic adapter already returns
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
- Only after approval construct `TurnControl::after_approval` with the
  active turn's `CancellationToken`.

## Execution

All pieces exist; the loop needs a turn-scoped owner for them:

- Launch via `browser::container` (validated spec, pinned image,
  argv-locked hardening flags) and attach `BrowserDriver` to the pipes.
- Execute through the provider adapters (`openai::computer_use::execute_call`,
  `anthropic::computer_use::execute_batch`) against the `TurnControl`.
- The existing Stop path (`src/cli/desktop_server/cancel.rs`, Desktop
  transport) must call `ContainerHandle::terminate` so delivery
  interrupted mid-action still removes the exact container. The runner
  now also exits on its own after any failed action or input close;
  treat that as backstop, not the primary path.

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

## Then

Prompt guidance (`vtcode-core/src/prompts/guidelines/`), tool schema
(`vtcode-utility-tool-specs`), `ThreadEvent` contract updates, budget /
presence tests, user guide plus quick reference, and the verified
model/action matrix — per the repo's major-feature docs rule. macOS
driver isolation and the Desktop Settings pane stay a separate release
gate behind signed-app acceptance.
