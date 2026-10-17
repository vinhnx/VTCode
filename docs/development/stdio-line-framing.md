# Bounded subprocess line framing

`vtcode-commons::line_framing::read_bounded_line` is the shared byte reader for
ACP and Copilot subprocess transports. It retains at most the supplied byte cap,
reuses the caller's buffer, and consumes the rest of an oversized physical line
through LF or EOF before returning. A truncated frame cannot leave its suffix
at the start of the next frame.

The caller selects delimiter handling explicitly:

| Adapter | Policy | Cap accounting | Return buffer |
| --- | --- | --- | --- |
| ACP stdio stdout and stderr | `ExcludeLf` | Counts content and CR, excludes LF | Existing owned `BoundedLine` |
| Copilot stdio stdout and stderr | `IncludeLf` | Counts content, CR, and LF | Reused caller buffer |
| Copilot server-client headers and stderr | `IncludeLf` | Counts content, CR, and LF | Reused caller buffer |

For `abc\n` with a three-byte cap, ACP retains `abc` without truncation;
Copilot retains `abc` with truncation because LF is the fourth byte. CR remains
content in both modes; existing Copilot adapters strip CR/LF when interpreting
headers or formatting diagnostics. JSON decoding and diagnostic sanitization
remain adapter responsibilities.

`None` means EOF without a physical line. `Some(false)` means a complete or
unterminated final line fit within the cap; `Some(true)` means retained content
was truncated. A zero cap still consumes a line, and an empty LF-only line is
returned even with `ExcludeLf`. The cap operates on bytes and may retain a
partial UTF-8 character; this helper does not decode text.

Read errors propagate unchanged. The helper is not cancellation-safe: an error
or dropped read future can leave the stream between frame boundaries. Existing
transport reader tasks terminate on read errors; callers must not restart a
fresh read on that stream and assume alignment. Pending-call cleanup, JSON-RPC
IDs, write-queue backpressure, child teardown, and protocol errors stay in the
respective transports.

## Verification

```bash
cargo nextest run --locked -p vtcode-commons -p vtcode-acp -p vtcode-llm --features vtcode-llm/copilot -E 'test(line_framing::tests) | test(transport::tests) | test(copilot::acp_client) | test(copilot::server_client)'
```

The shared tests compare framing against an independent complete-input oracle
across byte caps and buffered chunk sizes. Focused cases cover LF policy, CRLF,
empty lines, zero caps, oversized lines, EOF, non-ASCII bytes, buffer reuse, and
partial-read errors. Adapter regressions distinguish ACP and Copilot's exact-cap
behavior; existing transport tests cover cancellation, timeout, EOF, pending-call
cleanup, and notification routing.
