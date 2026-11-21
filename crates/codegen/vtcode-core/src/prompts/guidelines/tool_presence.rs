use super::*;

impl ToolPresence {
    pub(super) fn of(available_tools: &[String]) -> Self {
        // Single pass over the tool list: the old form ran one linear
        // `any()` scan per flag (10 scans per call, up to 4 calls per
        // model-aware prompt build). Same membership policy, one walk.
        let mut presence = Self {
            exec: false,
            write_stdin: false,
            code_search: false,
            web_search: false,
            read_file: false,
            grep_file: false,
            list_files: false,
            apply_patch: false,
            request_user_input: false,
            task_tracker: false,
            start_planning: false,
            matrix: false,
            record_decision: false,
            session_messaging: false,
            computer_use: false,
        };
        for tool in available_tools {
            match tool.as_str() {
                TOOL_EXEC_COMMAND => presence.exec = true,
                TOOL_WRITE_STDIN => presence.write_stdin = true,
                TOOL_CODE_SEARCH => presence.code_search = true,
                "web_search" => presence.web_search = true,
                TOOL_READ_FILE => presence.read_file = true,
                TOOL_GREP_FILE => presence.grep_file = true,
                TOOL_LIST_FILES => presence.list_files = true,
                TOOL_APPLY_PATCH => presence.apply_patch = true,
                TOOL_REQUEST_USER_INPUT => presence.request_user_input = true,
                TOOL_TASK_TRACKER => presence.task_tracker = true,
                TOOL_START_PLANNING => presence.start_planning = true,
                "matrix" => presence.matrix = true,
                TOOL_ASK_SESSION => presence.session_messaging = true,
                "record_decision" => presence.record_decision = true,
                TOOL_COMPUTER_USE => presence.computer_use = true,
                _ => {}
            }
        }
        presence
    }
}
