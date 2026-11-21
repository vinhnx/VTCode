# Computer Use Quick Reference

Fast lookup for the computer-use surface. Product behavior lives in the
[user guide](../user-guide/computer-use.md); integration seams live in the
[run-loop wiring notes](computer-use-runloop-wiring.md). The approved plan's authority
split holds: providers describe actions, the run loop owns turn identity/approval/cancellation,
drivers own target verification and input delivery.

## Config keys (`[computer_use]`)

| Key | Default | Purpose |
| --- | --- | --- |
| `enabled` | `false` | Master switch; needs a verified model. |
| `browser_build` | `null` | Absolute path to the approved static build served by the preview. |
| `image` | `vtcode-computer-use-browser:1.65.1` | Pinned container image (tag or digest). |
| `max_actions_per_turn` | `50` | Batch budget (1–1000). |
| `turn_timeout_secs` | `300` | Wall-clock turn budget (1–3600). |
| `decisions_review` | `false` | OpenAI Decisions screens each batch before approval (OpenAI routes only). |

## Verified matrix

`vtcode-computer-use/src/gate.rs` holds `VERIFIED_MATRIX` — one
`(provider, model, endpoint, protocol)` entry per accepted live probe. `verified_target_default`
denies unknown tuples before any network I/O, and both `generate_native`/`stream_native`
check it first. Current entry:

- OpenAI `gpt-6-luna` @ `https://api.openai.com/v1`, protocol `computer` (accepted 2026-11-19, `scripts/probe_openai_computer_use.py`)

Anthropic `computer_toolset_20260801` is implemented and contract-tested; add its entry
only after `scripts/probe_anthropic_computer_use.py` passes P1+P2 against the live endpoint.
The merge gateway mistranslates native tool declarations — probe direct endpoints.

## Route anatomy

**Inbound (model → loop).** Bound turns call `LLMProvider::generate_native` or the
streaming `LLMProvider::stream_native`; both declare the provider-native tool, decode
natives beside ordinaries into `NativeResponse` (`native_calls` with verbatim replay
items/blocks), and fail closed on malformed batches. Ordinary `generate`/`stream` still
reject native items (`reject_unbound_computer_item`, Anthropic `unbound_route_error`).

**Streaming.** `providers/openai/native_stream.rs` and `providers/anthropic/native_stream.rs`
accumulate SSE into the same `NativeResponse` the non-streaming parser produces
(parity by construction). Forwarded deltas are advisory (`NativeStreamEvent::TextDelta`
/`ReasoningDelta`); `Completed` is authoritative.

**Approval.** `request_target_approval` routes through the standard permission flow with
redacted routing-only args; the justification always prompts and defeats caching/auto-approval.
Denial, or any new control sequence after resume, starts over — no lease reuse, no replay.

**Execution.** `run_computer_turn` launches the hardened container, executes decoded
natives via `execute_decoded_call`/`execute_decoded` against the turn-scoped `TurnControl`,
and tears down through one funnel (finished/failed/stopped triggers recorded in the audit).

**Outbound (loop → next request).** Turn results flatten to a redacted text summary via
`prepare_tool_response_content`; native replay items are consumed within the turn and
dropped, so persisted history is all ordinary messages.

## Resume contract

Replayed computer-use history is ordinary: assistant `computer_use` call + redacted text
summary. Contract tests in `vtcode-llm/src/providers/{anthropic,openai}/request_builder/tests.rs`
(`resumed_computer_turn_*`, `interrupted_computer_turn_*`) assert valid replay with no
native items or screenshot bytes. Interrupted turns synthesize aborted results —
OpenAI `function_call_output` `"aborted"`, Anthropic `tool_result` `"aborted"` with
`is_error: true`. Fixtures: `providers/shared/native_resume.rs`; its `leaked_native_artifacts`
scan is the durable-output sentinel check.

## Events and redaction

`ThreadEvent` tool items carry summaries only (action kinds/counts, no typed text, no
base64). Screenshots never leave the dispatcher; `NativeResponse`/`NativeCall` are
in-memory, non-serializable, and their `Debug` impls expose routing metadata only.

## Probe scripts

| Script | Purpose |
| --- | --- |
| `scripts/probe_openai_computer_use.py` | P1 declare `{type:computer}`, expect `computer_call`; P2 stateless replay (`store=false`) with a real screenshot. |
| `scripts/probe_anthropic_computer_use.py` | P1 declare `{type:computer_toolset_20260801}`, expect computer toolset blocks; P2 replay `tool_use` + screenshot `tool_result`. |

Both print statuses, item/block types, and usage only — never credentials — and record
the accepted tuple for `VERIFIED_MATRIX` with the probe date.

## Hard limits

`MAX_SCREENSHOT_BYTES` 8 MiB, `MAX_BATCH_ACTIONS` 128, container runs
`--network=none --read-only --cap-drop=ALL --user=65534 --pids-limit=256 --memory=1g
--security-opt=no-new-privileges` with the build dir mounted read-only.
