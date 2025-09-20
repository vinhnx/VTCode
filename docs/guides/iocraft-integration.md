# Integrating `iocraft` into VTCode

## Why `iocraft`

[`iocraft`](https://github.com/ccbrown/iocraft) is a declarative terminal UI crate that combines a
React-like component system, flexbox layout via [`taffy`], rich styling, and hooks for async or
interactive experiences.

> Key features highlighted by the upstream README include:
> - Defining UIs with the `element!` macro and flexbox layouts powered by `taffy`.
> - Rendering styled output to any terminal, including fullscreen applications.
> - Creating interactive elements with async hooks and event handling.
> - Passing props and context by reference to avoid unnecessary cloning.
> - Supporting both Unix and Windows terminals for consistent visuals.

These capabilities map well to VTCode's needs for a chat-first REPL, tool-driven panes, and dynamic
context displays.

## Understand VTCode's Current Terminal Stack

Before refactoring, catalogue where VTCode manages presentation, input, and context so the
`iocraft` migration can happen incrementally.

| Responsibility | Current Location | Notes |
| --- | --- | --- |
| Session bootstrap + renderer ownership | `initialize_session` returns a `SessionState` owning an `AnsiRenderer` and placeholder metadata.【F:src/agent/runloop/unified/session_setup.rs†L20-L130】 | Inject the iocraft root here so the rest of the run loop receives component handles rather than raw writers. |
| Streaming response rendering | `stream_and_render_response` streams LLM tokens, pushes them into `AnsiRenderer`, and manages spinner lifecycle.【F:src/agent/runloop/unified/turn.rs†L169-L200】 | Map LLM events to iocraft state updates. |
| Prompt indicator, banner, tool summary | `display.rs` and `ui.rs` produce decorated lines using `AnsiRenderer` and theme styles.【F:src/agent/runloop/unified/display.rs†L1-L49】【F:src/agent/runloop/ui.rs†L1-L43】 | Recreate these as reusable components and context providers. |
| Chat input loop | `ChatInput::read_line` uses crossterm raw mode, handles editing, and triggers history scrolling callbacks.【F:src/agent/runloop/unified/input.rs†L1-L200】 | Replace with `iocraft` input events while preserving keyboard shortcuts and escape handling. |
| Transcript + context trimming | The transcript buffer persists output and feeds history views, while `ContextTrimConfig` governs LLM window management.【F:vtcode-core/src/utils/transcript.rs†L1-L57】【F:src/agent/runloop/context.rs†L1-L164】 | Expose these through iocraft context providers so UI components can react to changes. |
| Tool shell behavior | Helpers short-circuit user commands and display structured tool output.【F:src/agent/runloop/unified/shell.rs†L1-L119】 | Ensure tool panes stay synchronized with iocraft state. |

## Step-by-Step Integration Plan

### Step 1: Add `iocraft` dependencies

1. Add `iocraft = "*"` (pin to a vetted release) to `vtcode-core/Cargo.toml`, because the renderer lives in the core crate. Enable default features and include `smol` or `tokio` compatibility if required for hooks.
2. Propagate any async runtime bridge: expose a small adapter in `vtcode-core/src/ui/mod.rs` that abstracts over the runtime used by `ChatInput` so both iocraft and existing code can run in the same executor.【F:vtcode-core/src/ui/mod.rs†L1-L18】
3. Update `vtcode-core/src/lib.rs` exports if needed so the binary crate only imports new abstractions instead of the raw dependency.

### Step 2: Introduce an `iocraft` surface in `vtcode-core`

1. Create `vtcode-core/src/ui/iocraft.rs` with an `IocraftApp` struct that stores shared theme data, transcript handles, and runtime channels.
2. Re-export this module in `vtcode-core/src/ui/mod.rs` to keep the public UI surface cohesive.【F:vtcode-core/src/ui/mod.rs†L1-L18】
3. Translate theme colors from `theme::active_styles()` into `iocraft::Color` values so existing theming continues to work.【F:vtcode-core/src/utils/ansi.rs†L1-L146】

### Step 3: Build a component hierarchy

1. Implement a root component (`SessionRoot`) that renders:
   - A header banner using the same metadata currently written in `render_session_banner`.
   - A scrollable transcript view fed by the transcript buffer.
   - A footer containing the prompt indicator and input widget.【F:src/agent/runloop/ui.rs†L1-L43】【F:src/agent/runloop/unified/display.rs†L1-L49】
2. Each section should consume context providers representing the `SessionState`, `SessionStats`, and placeholder hints currently stored in structs inside `turn.rs` and `session_setup.rs`.
3. Replace `AnsiRenderer` usage with component-driven updates. Instead of `renderer.line(...)`, emit state mutations (e.g., push to a `Vec<String>` driving `Text` nodes) so `iocraft` handles drawing.【F:src/agent/runloop/unified/turn.rs†L40-L151】
4. Provide spinner and streaming feedback by toggling component state in response to `LLMStreamEvent` tokens, mirroring `spinner.finish_and_clear()` semantics.【F:src/agent/runloop/unified/turn.rs†L169-L200】

### Step 4: Wire the run loop to `iocraft`

1. When `initialize_session` constructs the `SessionState`, spawn the iocraft render loop (e.g., `element!(SessionRoot{...}).render_loop()`), and store communication channels inside `SessionState` so the rest of the loop can send updates.【F:src/agent/runloop/unified/session_setup.rs†L34-L130】
2. Convert functions that currently write directly to `AnsiRenderer` (`render_tool_output`, `SessionStats::render_summary`, etc.) to emit messages over these channels. The UI components read the same data via hooks and re-render automatically.【F:src/agent/runloop/unified/turn.rs†L40-L151】
3. Maintain transcript persistence by calling `transcript::append` in parallel until the iocraft port fully replaces history accessors.【F:vtcode-core/src/utils/transcript.rs†L1-L57】

### Step 5: Replace input handling

1. Rebuild `ChatInput` as an iocraft-controlled component using `TextInput` and event hooks.
2. Preserve keyboard shortcuts (scrolling, double-escape exit) by translating `KeyEvent` handlers from `handle_key` into the component's event map. Until the migration is complete, wrap the existing raw-mode reader inside an adapter that feeds events into the component tree.【F:src/agent/runloop/unified/input.rs†L37-L200】
3. Ensure scroll callbacks still manipulate `TranscriptView` state; this can become an iocraft hook that adjusts the offset and triggers re-rendering without manual cursor positioning.【F:src/agent/runloop/unified/turn.rs†L82-L157】

### Step 6: Context and tool management hooks

1. Expose `ContextTrimConfig` and `ContextTrimOutcome` through an application context so UI elements can display warnings (e.g., "context trimmed").【F:src/agent/runloop/context.rs†L1-L164】
2. Mirror tool execution updates (`derive_recent_tool_output`, session stats) in dedicated context providers or reducers that the UI reads to render tool panels and summaries.【F:src/agent/runloop/unified/shell.rs†L1-L119】【F:src/agent/runloop/unified/turn.rs†L40-L151】
3. Keep trimming logic untouched initially; once iocraft owns history rendering, you can streamline data flow by letting the UI subscribe directly to the message vector the LLM router mutates.【F:src/agent/runloop/context.rs†L60-L131】

### Step 7: Configuration and rollout controls

1. Introduce a feature flag in `vtcode.toml` (e.g., `ui.engine = "iocraft"`) and read it inside `run_single_agent_loop` to choose between the legacy renderer and the new component tree.【F:src/agent/runloop/mod.rs†L16-L23】
2. Gate unstable behaviors behind environment variables similar to the existing context limit override (`VTCODE_CONTEXT_TOKEN_LIMIT`).【F:src/agent/runloop/context.rs†L133-L164】
3. Document the migration path and add integration tests covering both code paths to prevent regressions in CI.

### Step 8: Testing checklist

- Exercise the chat REPL end-to-end (token streaming, slash commands, tool execution) with the `iocraft` UI enabled and disabled.
- Validate transcript pagination, context trimming alerts, and tool output rendering remain correct.
- Verify terminal resizing, theme persistence, and exit shortcuts still behave as expected.

Following these steps lets VTCode adopt `iocraft`'s declarative TUI primitives without rewriting the agent core, while preserving existing context management, chat run loop semantics, and tool integrations.
