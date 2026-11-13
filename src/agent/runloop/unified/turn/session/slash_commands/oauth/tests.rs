use super::*;
use hashbrown::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider as uni;
use vtcode_ui::tui::app::InlineHeaderContext;

use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::state::CtrlCSignal;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::agent::runloop::welcome::SessionBootstrap;
use vtcode_core::skills::types::Skill;

#[tokio::test]
async fn openrouter_login_returns_continue_when_ctrl_c_is_pending() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let turn = backing.turn_loop_context();
    assert!(matches!(turn.ctrl_c_state.register_signal(), CtrlCSignal::Cancel));

    let header_context = Box::leak(Box::new(InlineHeaderContext::default()));
    let model_picker_state = Box::leak(Box::new(None::<ModelPickerState>));
    let palette_state = Box::leak(Box::new(None::<ActivePalette>));
    let conversation_history = Box::leak(Box::new(Vec::<uni::Message>::new()));
    let loaded_skills = Box::leak(Box::new(Arc::new(RwLock::new(HashMap::<String, Skill>::new()))));
    let linked_directories = Box::leak(Box::new(Vec::<LinkedDirectory>::new()));
    let session_bootstrap = Box::leak(Box::new(SessionBootstrap::default()));
    let vt_cfg = Box::leak(Box::new(None::<VTCodeConfig>));
    let async_mcp_manager = None;
    let checkpoint_manager = None;
    let lifecycle_hooks = None;
    let harness_emitter = None;
    let (webmcp_prompt_sender, _webmcp_prompt_receiver) = crate::agent::runloop::unified::webmcp::prompt_channel();
    let webmcp_bridge = Box::leak(Box::new(None));
    let active_primary_agent = Box::leak(Box::new(vtcode_core::primary_agent::ActivePrimaryAgentState::default()));
    let plan_session = Box::leak(Box::new(
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default(),
    ));
    let thread_handle = Box::leak(Box::new(
        vtcode_core::core::threads::ThreadManager::new()
            .start_thread_with_identifier("test-thread", vtcode_core::core::threads::ThreadBootstrap::new(None)),
    ));

    let ctx = SlashCommandContext {
        thread_id: "test-thread",
        active_thread_label: "main",
        thread_handle,
        renderer: turn.renderer,
        handle: turn.handle,
        session: turn.session,
        header_context,
        config: turn.config,
        vt_cfg,
        provider_client: turn.provider_client,
        session_bootstrap,
        model_picker_state,
        palette_state,
        tool_registry: turn.tool_registry,
        conversation_history,
        decision_ledger: turn.decision_ledger,
        context_manager: turn.context_manager,
        active_primary_agent,
        session_stats: turn.session_stats,
        plan_session,
        input_status_state: turn.input_status_state,
        tools: turn.tools,
        tool_catalog: turn.tool_catalog,
        async_mcp_manager,
        mcp_panel_state: turn.mcp_panel_state,
        linked_directories,
        ctrl_c_state: turn.ctrl_c_state,
        ctrl_c_notify: turn.ctrl_c_notify,
        full_auto: turn.full_auto,
        webmcp_prompt_sender: &webmcp_prompt_sender,
        webmcp_bridge,
        loaded_skills,
        checkpoint_manager,
        lifecycle_hooks,
        harness_emitter,
    };

    let control = handle_oauth_login(ctx, OPENROUTER_PROVIDER.to_string())
        .await
        .expect("openrouter login should continue after cancel");

    assert!(matches!(control, SlashCommandControl::Continue));
}
