extract vtcode-\* module as separate crates like tui-shimmer.

--

improve and defer stop loading state for status bar until task all done, not just after first response from LLM.

--

pub(crate) fn resolve_max_tool_retries(
\_tool_name: &str,
\_vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
) -> usize {
// TODO: Re-implement per-tool retry configuration once config structure is verified.
// Currently AgentConfig does not expose a 'tools' map.
3
}
