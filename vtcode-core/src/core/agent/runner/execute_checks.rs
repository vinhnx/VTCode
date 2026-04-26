use super::AgentRunner;
use crate::utils::colors::style;

impl AgentRunner {
    pub(super) fn warn_on_empty_response(
        &self,
        agent_prefix: &str,
        response_content: &str,
        has_tool_calls: bool,
    ) {
        if response_content.trim().is_empty() && !has_tool_calls {
            self.runner_println(format_args!(
                "{} {} received empty response with no tool calls",
                agent_prefix,
                style("(WARN)").red().bold()
            ));
        }
    }
}
