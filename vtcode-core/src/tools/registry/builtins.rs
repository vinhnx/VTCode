use std::path::PathBuf;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::tools::ask_user_question::AskUserQuestionTool;
use crate::tools::handlers::{EnterPlanModeTool, ExitPlanModeTool, PlanModeState};
use crate::tools::request_user_input::RequestUserInputTool;

use super::progressive_docs::minimal_tool_signatures;
use super::registration::ToolRegistration;
use super::{ToolInventory, ToolRegistry};

/// Register all builtin tools into the inventory using the shared plan mode state.
pub(super) fn register_builtin_tools(inventory: &ToolInventory, plan_mode_state: &PlanModeState) {
    for registration in builtin_tool_registrations(Some(plan_mode_state)) {
        let tool_name = registration.name();
        if let Err(err) = inventory.register_tool(registration) {
            eprintln!("Warning: Failed to register tool '{}': {}", tool_name, err);
        }
    }
}

/// Build builtin tool registrations. In metadata-only contexts (e.g., declaration building),
/// callers may pass `None`, and a placeholder PlanModeState will be used.
pub(super) fn builtin_tool_registrations(
    plan_mode_state: Option<&PlanModeState>,
) -> Vec<ToolRegistration> {
    let plan_state = plan_mode_state
        .cloned()
        .unwrap_or_else(|| PlanModeState::new(PathBuf::new()));

    let sigs = minimal_tool_signatures();

    let mut registrations = vec![
        // ============================================================
        // HUMAN-IN-THE-LOOP (HITL)
        // ============================================================
        ToolRegistration::from_tool_instance(
            tools::ASK_USER_QUESTION,
            CapabilityLevel::Basic,
            AskUserQuestionTool,
        )
        .with_llm_visibility(false),
        ToolRegistration::from_tool_instance(
            tools::REQUEST_USER_INPUT,
            CapabilityLevel::Basic,
            RequestUserInputTool,
        )
        .with_aliases([tools::ASK_QUESTIONS, "askQuestions"]),
        // ============================================================
        // PLAN MODE (enter/exit)
        // ============================================================
        ToolRegistration::from_tool_instance(
            tools::ENTER_PLAN_MODE,
            CapabilityLevel::Basic,
            EnterPlanModeTool::new(plan_state.clone()),
        )
        .with_aliases(["plan_mode", "enter_plan", "start_planning"]),
        ToolRegistration::from_tool_instance(
            tools::EXIT_PLAN_MODE,
            CapabilityLevel::Basic,
            ExitPlanModeTool::new(plan_state.clone()),
        )
        .with_aliases([
            "exit_plan",
            "plan_exit",
            "start_implementation",
            "implement_plan",
        ]),
        // ============================================================
        // SEARCH & DISCOVERY (1 tool - unified)
        // ============================================================
        ToolRegistration::new(
            tools::UNIFIED_SEARCH,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::unified_search_executor,
        )
        .with_aliases([
            tools::GREP_FILE,
            tools::LIST_FILES,
            tools::CODE_INTELLIGENCE,
            tools::AGENT_INFO,
            tools::WEB_FETCH,
            tools::SKILL,
            tools::SEARCH_TOOLS,
            tools::SEARCH,
            tools::FIND,
        ]),
        // ============================================================
        // SHELL EXECUTION (1 tool - unified)
        // ============================================================
        ToolRegistration::new(
            tools::UNIFIED_EXEC,
            CapabilityLevel::Bash,
            true,
            ToolRegistry::unified_exec_executor,
        )
        .with_aliases([
            tools::RUN_PTY_CMD,
            tools::EXECUTE_CODE,
            tools::CREATE_PTY_SESSION,
            tools::LIST_PTY_SESSIONS,
            tools::CLOSE_PTY_SESSION,
            tools::SEND_PTY_INPUT,
            tools::READ_PTY_SESSION,
            tools::EXEC_PTY_CMD,
            tools::EXEC,
            tools::SHELL,
        ]),
        // ============================================================
        // FILE OPERATIONS (1 tool - unified)
        // ============================================================
        ToolRegistration::new(
            tools::UNIFIED_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::unified_file_executor,
        )
        .with_aliases([
            tools::READ_FILE,
            tools::WRITE_FILE,
            tools::DELETE_FILE,
            tools::EDIT_FILE,
            tools::APPLY_PATCH,
            tools::CREATE_FILE,
            tools::MOVE_FILE,
            tools::COPY_FILE,
            tools::FILE_OP,
        ]),
        // ============================================================
        // INTERNAL TOOLS (Hidden from LLM, used by unified tools)
        // ============================================================
        ToolRegistration::new(
            tools::READ_FILE,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::read_file_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::WRITE_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::write_file_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::EDIT_FILE,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::edit_file_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::GREP_FILE,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::grep_file_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::LIST_FILES,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::list_files_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::RUN_PTY_CMD,
            CapabilityLevel::Bash,
            true,
            ToolRegistry::run_pty_cmd_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::SEND_PTY_INPUT,
            CapabilityLevel::Bash,
            true,
            ToolRegistry::send_pty_input_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::READ_PTY_SESSION,
            CapabilityLevel::Bash,
            true,
            ToolRegistry::read_pty_session_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::LIST_PTY_SESSIONS,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::list_pty_sessions_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::CLOSE_PTY_SESSION,
            CapabilityLevel::Bash,
            false,
            ToolRegistry::close_pty_session_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::CODE_INTELLIGENCE,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::code_intelligence_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::GET_ERRORS,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::get_errors_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::AGENT_INFO,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::agent_info_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::SEARCH_TOOLS,
            CapabilityLevel::CodeSearch,
            false,
            ToolRegistry::search_tools_executor,
        )
        .with_llm_visibility(false),
        ToolRegistration::new(
            tools::APPLY_PATCH,
            CapabilityLevel::Editing,
            false,
            ToolRegistry::apply_patch_executor,
        )
        .with_llm_visibility(false),
        // ============================================================
        // SKILL MANAGEMENT TOOLS (3 tools)
        // ============================================================
        // Note: These tools are created dynamically in session_setup.rs
        // because they depend on runtime context (skills map, tool registry).
        // They are NOT registered here; instead they are registered
        // on-demand in session initialization.
        //
        // Tools created in session_setup.rs:
        // - list_skills
        // - load_skill
        // - load_skill_resource
        // - spawn_subagent
    ];

    // Apply descriptions from signatures where available
    for reg in &mut registrations {
        if let Some(sig) = sigs.get(reg.name()) {
            *reg = reg.clone().with_description(sig.brief);
        }
    }

    registrations
}
