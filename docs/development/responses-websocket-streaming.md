# Responses WebSocket streaming

`provider.openai.websocket_mode = true` enables persistent Responses WebSocket transport for generation and
both normalized and legacy streaming APIs on eligible native OpenAI and compatible Responses endpoints.
The option defaults to `false`; unsupported backends retain HTTP transport. WebSocket events always use
`store = false`.

TLS connections explicitly select Rustls's Ring crypto provider and the bundled WebPKI trust roots.
This avoids a startup panic when other dependencies enable both Ring and AWS-LC; certificate and
hostname verification remain enabled, and TLS failures use the normal transport error path.
With tracing enabled at `info`, connection establishment and the completed response's recognized
server-reported `service_tier` are recorded without prompt text or credentials. The configured tier
in the TUI expresses the requested tier; the response log provides evidence of the tier actually served.
Completion logging happens once at the transport parsing boundary; rebuilding continuation history emits no
additional completion records.
Native API requests omit the unsupported `output_types` extension. A generic `invalid_request_error`
or parameter rejection (including a generic vendor error code) does not prove that Responses is unavailable.
These errors must retain their original diagnostic and leave the model's route eligible instead of
downgrading to Chat Completions. WebSocket startup failures are visible at warning level.

## Inline file inputs

Local non-image attachments such as `@README.md` use `input_file` content parts.
The Responses serializer wraps raw base64 in a MIME-prefixed data URL, for example
`data:text/markdown;base64,...`; already-prefixed data URLs, file IDs, and remote file URLs
retain their existing representation. This applies to HTTP and WebSocket requests, including
replayed history, without changing stored messages. Size validation counts decoded bytes.
See the [official OpenAI file-input guide](https://developers.openai.com/api/docs/guides/file-inputs).

## Lifecycle and cancellation

One response lane serializes generation and streaming on the connection. A lease removes the socket and
continuation cache from their reusable slots before startup. A returned stream owns that lease and the shared
Responses processor; it needs no provider borrow or detached background task.

The first request warms up with `response.create` and `generate = false`. After the warmup completes, the
generated event chains from its response ID. Streaming startup reads lifecycle events until the first
deliverable text, reasoning, tool-call event, or validated completion. Ping frames receive pong replies;
control frames never become model events.

Only `response.completed` permits successful final assembly and socket restoration. The shared processor
preserves text, refusal, reasoning classification, provider tool-call IDs and arguments, usage, and recovery
from empty final output when usable deltas arrived. Failure, incomplete response, malformed events, close,
EOF, deadline expiry, cancellation, or early stream drop invalidate the socket and its continuation cache.
The lease stays active through final usage delivery until `Done` is delivered; dropping between those events
invalidates the connection even when the server has already completed.
Dropping a future while it waits for the lane leaves the active response alone.

Independent tool-output probe generation uses `LLMProvider::generate_auxiliary` over HTTP, including HTTP SSE
for streaming-required models. It never leases the main WebSocket or replaces its response ID. Auxiliary completion,
failure, and cancellation therefore preserve the socket and continuation needed for the next tool result.

Connections have a 30-second timeout. Each WebSocket request has one deadline from
`timeouts.streaming_ceiling_seconds`, including lane acquisition, connection, warmup, and generation; a zero
setting uses 600 seconds. Receiving lifecycle or control frames does not reset this deadline.

## Continuation

The cache records request input plus completed assistant and tool-call output in the history builder's
canonical replay representation, after applying the model's reasoning policy just as callers do.
History comparisons ignore only `prompt_cache_breakpoint` annotations.
Matching model, instructions, tools, and a complete history prefix permits `previous_response_id` with
only genuinely new input, such as tool results or the next user message. Completed assistant text and
tool calls are not resent.

Changed model, instructions, tools, edited or compacted history, and uncertain output matching start a
full-input chain. Empty authoritative output, opaque output, or phase metadata that cannot be recovered
through the response contract disable continuation for that response. Reconnecting always drops
connection-local response IDs.

This follows the incremental-input and warmup contracts in the official
[OpenAI WebSocket guide](https://developers.openai.com/api/docs/guides/websocket-mode).

## Fallback and service tiers

Before a deliverable event reaches the caller, any WebSocket startup failure clears transport state and
uses HTTP SSE for that request, without another WebSocket attempt. This includes active-response,
connection-limit, and missing-previous-response errors. A later request may reconnect.

After a text, reasoning, or tool-call event has been exposed, a failure emits a stream error without HTTP
replay. The legacy adapter retains its existing event enum and carries tools and usage in the final response.
It applies the model's reasoning policy after final assembly, including recovery from retained deltas.
Because tool deltas are assembled into the legacy final response, failures during tool-only startup still
use HTTP until a legacy event reaches the caller. Normalized tool-start events cross that boundary immediately.

The model-scoped tier rejection cache applies before WebSocket and HTTP requests. Every warmup and
generated event forwards the effective service tier, including `ultrafast`, as required by the official
[Ultrafast guide](https://developers.openai.com/api/docs/guides/ultrafast-mode). Structured WebSocket tier
rejections are classified before diagnostic formatting, cached, and routed to tier-less HTTP. Both HTTP
streaming entrypoints retry a confirmed rejection once without the tier, retaining authentication,
turn metadata, and a fresh client request ID. A failed retry uses the existing error classification and
transport fallback policy: an unsupported Responses endpoint can fall back to Chat Completions when
allowed, while a required Responses route surfaces the error. The tier retry remains bounded to one attempt.

## Request builder ownership

HTTP and WebSocket transports retain the same request-builder entrypoints. Private modules under
`crates/codegen/vtcode-llm/src/providers/openai/` separate the wire payload from history and model policy:

| Source | Responsibility |
| --- | --- |
| `request_builder.rs` | Shared contexts and Chat Completions/Responses JSON assembly |
| `request_builder/model_policy.rs` | Model-family predicates, reasoning/verbosity defaults, and sampling policy |
| `request_builder/history.rs` | Replay instructions, assistant phase filtering, dynamic reminders, and cache breakpoints |
| `request_builder/tests.rs` | Public-builder regressions for payload, history, and backend boundaries |

Keep policy precedence and backend capability flags explicit. The history builder must preserve opaque compaction
state and keep volatile reminders outside the stable instruction prefix. Tests remain at their original
`providers::openai::request_builder::tests` path.

## Local verification

Scripted local servers assert ordered deltas, exact continuation inputs, request counts, tier forwarding,
fallback boundaries, cancellation, serialized generation, and ping/deadline behavior. No paid API calls
are required.

```sh
cargo nextest run --locked -p vtcode-llm -E 'test(websocket) | test(service_tier) | test(responses_stream) | test(responses_adapter) | test(stream_decoder) | test(providers::openai::request_builder::)'
cargo nextest run --locked -p vtcode-config -E 'test(openai_config)'
cargo check --locked -p vtcode-llm -p vtcode-config
cargo clippy --locked -p vtcode-llm -p vtcode-config --all-targets -- -D warnings
cargo fmt --all -- --check
./scripts/check-dev.sh
```
