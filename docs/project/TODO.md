extract vtcode-\* module as separate crates like tui-shimmer.

--

improve and defer stop loading state for status bar until task all done, not just after first response from LLM.

--

Review the overall

@tool_dispatch.rs

@call.rs

@turn/

@unified/

@runloop

module and its recent changes with meticulous attention to architectural integrity, identifying opportunities to improve stability, resilience, and reliability of the execution harness while eliminating redundancy and enforcing DRY principles throughout the implementation.

## --

pub(crate) fn resolve_max_tool_retries(
\_tool_name: &str,
\_vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
) -> usize {
// TODO: Re-implement per-tool retry configuration once config structure is verified.
// Currently AgentConfig does not expose a 'tools' map.
3
}

--

fix debug logs causing tui broken in the bottom view



──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
Build something (@files, #prompts, /commands or Shift+Tab to switch to modes)
──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
⠋ Executing run_pty_cmd... (Press Ctrl+C to cancel)vtcode(86761) MallocStackLogging: can't turn off malloc stack logging because it was not enabled.m2.1:cloud | (low)
Ghostty main*