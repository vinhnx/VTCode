//! Headless fallbacks for interactive slash commands.
//!
//! The desktop service runs the shared runloop without inline UI. Every
//! handler that opens a modal, picker, or confirmation must degrade to text
//! instead of parking state or waiting forever: a parked palette wedges later
//! modal commands, and a wait wedges the turn until cancel. Each test below
//! races the handler against a timeout, so a regression fails fast instead of
//! hanging the suite.

use super::handlers::{
    handle_clear_screen, handle_manage_skills, handle_oauth_login, handle_open_donate_links,
    handle_run_instruction_audit, handle_show_log_viewer, handle_show_memory_config, handle_show_permissions,
    handle_show_settings_reset, handle_show_transcript_stats, handle_start_mode_palette, handle_start_session_palette,
    handle_start_theme_palette, handle_toggle_planning_workflow,
};
use super::secrets::handle_manage_secrets;
use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::slash_commands::{
    LogFormat, LogScope, SecretCommandAction, SessionPaletteMode, ThemePaletteMode,
};
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::agent::runloop::welcome::SessionBootstrap;
use hashbrown::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::timeout;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::skills::types::Skill;
use vtcode_tui_runtime::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::InlineHeaderContext;

const HEADLESS_TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Handler context over a headless renderer, mirroring the desktop service.
/// Leaked parts follow the `oauth/tests.rs` precedent; the harness keeps the
/// owned pieces alive while a test drives one handler at a time.
struct HeadlessHarness {
    backing: TestTurnProcessingBacking,
    renderer: AnsiRenderer,
    header_context: &'static mut InlineHeaderContext,
    model_picker_state: &'static mut Option<ModelPickerState>,
    palette_state: &'static mut Option<ActivePalette>,
    conversation_history: &'static mut Vec<uni::Message>,
    loaded_skills: &'static mut Arc<RwLock<HashMap<String, Skill>>>,
    linked_directories: &'static mut Vec<LinkedDirectory>,
    session_bootstrap: &'static mut SessionBootstrap,
    vt_cfg: &'static mut Option<VTCodeConfig>,
    webmcp_prompt_sender: tokio::sync::mpsc::Sender<String>,
    _webmcp_prompt_receiver: tokio::sync::mpsc::Receiver<String>,
    webmcp_bridge: &'static mut Option<crate::agent::runloop::unified::webmcp::ActiveWebmcpBridge>,
    active_primary_agent: &'static mut vtcode_core::primary_agent::ActivePrimaryAgentState,
    plan_session: &'static mut crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState,
    thread_handle: &'static mut vtcode_core::core::threads::ThreadRuntimeHandle,
    goal_tracker: super::SharedGoalTracker,
}

impl HeadlessHarness {
    async fn new() -> Self {
        let (webmcp_prompt_sender, webmcp_prompt_receiver) = crate::agent::runloop::unified::webmcp::prompt_channel();
        let renderer = AnsiRenderer::stdout();
        assert!(!renderer.supports_inline_ui(), "fixture renderer must be headless");
        Self {
            backing: TestTurnProcessingBacking::new().await,
            renderer,
            header_context: Box::leak(Box::new(InlineHeaderContext::default())),
            model_picker_state: Box::leak(Box::new(None::<ModelPickerState>)),
            palette_state: Box::leak(Box::new(None::<ActivePalette>)),
            conversation_history: Box::leak(Box::new(Vec::<uni::Message>::new())),
            loaded_skills: Box::leak(Box::new(Arc::new(RwLock::new(HashMap::<String, Skill>::new())))),
            linked_directories: Box::leak(Box::new(Vec::<LinkedDirectory>::new())),
            session_bootstrap: Box::leak(Box::new(SessionBootstrap::default())),
            vt_cfg: Box::leak(Box::new(None::<VTCodeConfig>)),
            webmcp_prompt_sender,
            _webmcp_prompt_receiver: webmcp_prompt_receiver,
            webmcp_bridge: Box::leak(Box::new(None)),
            active_primary_agent: Box::leak(Box::new(vtcode_core::primary_agent::ActivePrimaryAgentState::default())),
            plan_session: Box::leak(Box::new(
                crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default(),
            )),
            thread_handle: Box::leak(Box::new(
                vtcode_core::core::threads::ThreadManager::new().start_thread_with_identifier(
                    "test-thread",
                    vtcode_core::core::threads::ThreadBootstrap::new(None),
                ),
            )),
            goal_tracker: Arc::new(tokio::sync::Mutex::new(vtcode_memory::GoalTracker::new(
                std::env::temp_dir().join("vtcode-goal-test-noop"),
            ))),
        }
    }

    fn ctx(&mut self) -> SlashCommandContext<'_> {
        let turn = self.backing.turn_loop_context();
        SlashCommandContext {
            thread_id: "test-thread",
            active_thread_label: "main",
            thread_handle: &mut *self.thread_handle,
            renderer: &mut self.renderer,
            handle: turn.handle,
            session: turn.session,
            header_context: &mut *self.header_context,
            config: turn.config,
            vt_cfg: &mut *self.vt_cfg,
            provider_client: turn.provider_client,
            session_bootstrap: &mut *self.session_bootstrap,
            model_picker_state: &mut *self.model_picker_state,
            palette_state: &mut *self.palette_state,
            tool_registry: turn.tool_registry,
            conversation_history: &mut *self.conversation_history,
            decision_ledger: turn.decision_ledger,
            context_manager: turn.context_manager,
            active_primary_agent: &mut *self.active_primary_agent,
            session_stats: turn.session_stats,
            plan_session: &mut *self.plan_session,
            goal_tracker: &self.goal_tracker,
            input_status_state: turn.input_status_state,
            tools: turn.tools,
            tool_catalog: turn.tool_catalog,
            async_mcp_manager: None,
            mcp_panel_state: turn.mcp_panel_state,
            linked_directories: &mut *self.linked_directories,
            ctrl_c_state: turn.ctrl_c_state,
            ctrl_c_notify: turn.ctrl_c_notify,
            full_auto: turn.full_auto,
            webmcp_prompt_sender: &self.webmcp_prompt_sender,
            webmcp_bridge: &mut *self.webmcp_bridge,
            loaded_skills: &mut *self.loaded_skills,
            checkpoint_manager: None,
            lifecycle_hooks: None,
            harness_emitter: turn.harness_emitter,
        }
    }

    fn palette_state(&self) -> &Option<ActivePalette> {
        self.palette_state
    }
}

async fn expect_continue(
    outcome: Result<Result<SlashCommandControl, anyhow::Error>, tokio::time::error::Elapsed>,
    what: &str,
) {
    let control = outcome
        .unwrap_or_else(|_| panic!("{what} must not wait on a headless modal"))
        .expect(what);
    assert!(matches!(control, SlashCommandControl::Continue), "{what} must continue");
}

#[tokio::test]
async fn headless_mode_palette_lists_without_parking_modal_state() {
    let mut harness = HeadlessHarness::new().await;
    let control = timeout(HEADLESS_TEST_TIMEOUT, handle_start_mode_palette(harness.ctx()))
        .await
        .unwrap_or_else(|_| panic!("mode list must not wait headless"))
        .expect("mode handler");
    assert!(matches!(control, SlashCommandControl::Continue));
    assert!(harness.palette_state().is_none(), "mode list must not park modal state");
}

#[tokio::test]
async fn explicit_plan_on_promotes_existing_suggested_planning() {
    use vtcode_core::core::interfaces::session::PlanningEntrySource;

    let mut harness = HeadlessHarness::new().await;
    harness.backing.enable_planning();
    harness.plan_session.enter(PlanningEntrySource::AgentSuggestion);
    assert!(!harness.plan_session.requires_explicit_plan_approval());

    expect_continue(
        timeout(HEADLESS_TEST_TIMEOUT, handle_toggle_planning_workflow(harness.ctx(), Some(true))).await,
        "explicit plan on",
    )
    .await;
    assert!(harness.plan_session.requires_explicit_plan_approval());
}

#[tokio::test]
async fn plan_task_is_not_an_approval() {
    let mut harness = HeadlessHarness::new().await;
    harness.backing.enable_planning();
    let event_path = harness.backing.enable_harness_emitter();
    harness.backing.persist_plan_for_test(
        "## Summary\nRepair the parser.\n## Implementation Steps\n1. Validate input -> files: [src/parser.rs] -> verify: [cargo check --locked]\n## Test Cases and Validation\n- cargo check --locked exits 0.\n## Assumptions and Defaults\n- Preserve the API.",
    ).await;
    harness.plan_session.recovery.research_closed = true;
    harness.plan_session.recovery.outcome = vtcode_core::exec::events::planning::PlanningOutcome::AwaitingInput;
    harness.plan_session.recovery.best_draft = Some("old draft".to_owned());
    harness.plan_session.set_previous_primary_agent(Some("build".to_owned()));
    harness
        .conversation_history
        .push(uni::Message::system("Planning recovery: old directive".to_owned()));
    let control = super::handle_outcome(
        super::SlashCommandOutcome::TogglePlanningWorkflow {
            enable: Some(true),
            prompt: Some("review how users approve requests".to_owned()),
        },
        harness.ctx(),
    )
    .await
    .expect("plan task");
    assert!(matches!(control, SlashCommandControl::SubmitPrompt(_)));
    assert!(!harness.plan_session.recovery.research_closed);
    assert!(harness.plan_session.recovery.best_draft.is_none());
    assert_eq!(harness.plan_session.restore_agent_after_planning(), Some("build"));
    assert!(harness.conversation_history.is_empty());
    assert!(harness.ctx().tool_registry.planning_workflow_state().get_plan_file().is_none());
    let events = std::fs::read_to_string(event_path).expect("planning checkpoint");
    assert!(events.contains("planning-recovery"));
    assert!(!events.contains("old draft"));
    harness.plan_session.recovery.outcome = vtcode_core::exec::events::planning::PlanningOutcome::AwaitingApproval;
    harness.plan_session.begin_explicit_recovery_cycle_for_user_input();
    assert!(harness.plan_session.take_recovery_input_pending());
}

#[tokio::test]
async fn headless_oauth_login_directs_browser_flows_to_a_terminal() {
    for provider in ["openai", "openrouter"] {
        let mut harness = HeadlessHarness::new().await;
        expect_continue(
            timeout(HEADLESS_TEST_TIMEOUT, handle_oauth_login(harness.ctx(), provider.to_string())).await,
            "oauth login",
        )
        .await;
    }
}

#[tokio::test]
async fn headless_permissions_reports_effective_mode() {
    let mut harness = HeadlessHarness::new().await;
    expect_continue(timeout(HEADLESS_TEST_TIMEOUT, handle_show_permissions(harness.ctx())).await, "permissions").await;
}

#[tokio::test]
async fn headless_clear_reports_nothing_cleared() {
    let mut harness = HeadlessHarness::new().await;
    expect_continue(timeout(HEADLESS_TEST_TIMEOUT, handle_clear_screen(harness.ctx())).await, "clear").await;
}

#[tokio::test]
async fn headless_transcript_stats_omits_tui_memory_note() {
    let mut harness = HeadlessHarness::new().await;
    expect_continue(timeout(HEADLESS_TEST_TIMEOUT, handle_show_transcript_stats(harness.ctx())).await, "transcript")
        .await;
}

#[tokio::test]
async fn headless_donate_lists_both_links_without_a_picker() {
    let mut harness = HeadlessHarness::new().await;
    expect_continue(timeout(HEADLESS_TEST_TIMEOUT, handle_open_donate_links(harness.ctx())).await, "donate").await;
}

#[tokio::test]
async fn headless_config_reset_points_to_the_terminal_command() {
    let mut harness = HeadlessHarness::new().await;
    expect_continue(timeout(HEADLESS_TEST_TIMEOUT, handle_show_settings_reset(harness.ctx())).await, "reset").await;
}

#[tokio::test]
async fn headless_config_memory_renders_lines_without_a_modal() {
    let mut harness = HeadlessHarness::new().await;
    expect_continue(timeout(HEADLESS_TEST_TIMEOUT, handle_show_memory_config(harness.ctx())).await, "memory").await;
}

#[tokio::test]
async fn headless_config_log_names_the_terminal_alternative() {
    let mut harness = HeadlessHarness::new().await;
    expect_continue(
        timeout(
            HEADLESS_TEST_TIMEOUT,
            handle_show_log_viewer(harness.ctx(), LogFormat::Text, LogScope::Thread, false),
        )
        .await,
        "log viewer",
    )
    .await;
}

#[tokio::test]
async fn headless_skills_doctor_renders_populated_report_without_waiting() {
    use crate::agent::runloop::SkillCommandAction;

    let mut harness = HeadlessHarness::new().await;
    {
        let ctx = harness.ctx();
        let collector = ctx.tool_registry.metrics_collector();
        collector.record_skill_created("drill".to_string(), "agent_skill".to_string());
        collector.record_skill_execution("drill".to_string(), 42, true);
    }
    expect_continue(
        timeout(HEADLESS_TEST_TIMEOUT, handle_manage_skills(harness.ctx(), SkillCommandAction::Doctor)).await,
        "/skills doctor",
    )
    .await;
    assert!(harness.palette_state().is_none(), "/skills doctor must not park modal state");
}

#[tokio::test]
async fn headless_doctor_prompt_audit_renders_without_waiting() {
    let mut harness = HeadlessHarness::new().await;
    expect_continue(
        timeout(HEADLESS_TEST_TIMEOUT, handle_run_instruction_audit(harness.ctx())).await,
        "/doctor prompt-audit",
    )
    .await;
    assert!(harness.palette_state().is_none(), "/doctor prompt-audit must not park modal state");
}

#[tokio::test]
async fn headless_secret_add_points_to_settings_or_terminal_without_waiting() {
    let mut harness = HeadlessHarness::new().await;
    expect_continue(
        timeout(
            HEADLESS_TEST_TIMEOUT,
            handle_manage_secrets(
                harness.ctx(),
                SecretCommandAction::Add { provider: "openai".to_string(), key_name: None },
            ),
        )
        .await,
        "/secret add",
    )
    .await;
    assert!(harness.palette_state().is_none(), "/secret add must not park modal state");
}

#[tokio::test]
async fn headless_secret_delete_points_to_terminal_without_waiting() {
    let mut harness = HeadlessHarness::new().await;
    expect_continue(
        timeout(
            HEADLESS_TEST_TIMEOUT,
            handle_manage_secrets(
                harness.ctx(),
                SecretCommandAction::Delete { provider: "openai".to_string(), key_name: None },
            ),
        )
        .await,
        "/secret delete",
    )
    .await;
    assert!(harness.palette_state().is_none(), "/secret delete must not park modal state");
}

#[tokio::test]
async fn headless_theme_palette_lists_without_parking_modal_state() {
    let mut harness = HeadlessHarness::new().await;
    expect_continue(
        timeout(HEADLESS_TEST_TIMEOUT, handle_start_theme_palette(harness.ctx(), ThemePaletteMode::Select)).await,
        "theme list",
    )
    .await;
    assert!(harness.palette_state().is_none(), "theme list must not park modal state");
}

#[tokio::test]
async fn headless_session_palette_reports_guidance_without_parking_modal_state() {
    for mode in [SessionPaletteMode::Resume, SessionPaletteMode::Fork] {
        let mut harness = HeadlessHarness::new().await;
        expect_continue(
            timeout(HEADLESS_TEST_TIMEOUT, handle_start_session_palette(harness.ctx(), mode, 5, false)).await,
            "session list",
        )
        .await;
        assert!(harness.palette_state().is_none(), "session list must not park modal state");
    }
}

#[tokio::test]
async fn settings_secrets_slash_routes_share_palette_and_refuse_conflicts() {
    use super::outcome_router::route_outcome;
    use crate::agent::runloop::slash_commands::handle_slash_command;
    use crate::agent::runloop::unified::settings_interactive::secrets::SECRETS_VIEW;
    use vtcode_ui::tui::app::{InlineCommand, InlineHandle, TransientRequest};

    let mut harness = HeadlessHarness::new().await;
    let workspace = tempfile::tempdir().expect("temporary workspace");
    harness.ctx().config.workspace = workspace.path().to_path_buf();

    for command in ["secret", "config secrets"] {
        let outcome = handle_slash_command(command, &mut harness.renderer, workspace.path())
            .await
            .expect("parse Secrets route");
        expect_continue(timeout(HEADLESS_TEST_TIMEOUT, route_outcome(outcome, harness.ctx())).await, command).await;
        assert!(harness.palette_state().is_none(), "headless Secrets should finish without a palette");
    }

    let (tx, mut commands) = tokio::sync::mpsc::unbounded_channel();
    harness.renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(tx), Default::default());
    for command in ["secret", "config secrets"] {
        *harness.palette_state = Some(ActivePalette::Mode);
        let outcome = handle_slash_command(command, &mut harness.renderer, workspace.path())
            .await
            .unwrap();
        route_outcome(outcome, harness.ctx()).await.unwrap();
        assert!(matches!(harness.palette_state(), Some(ActivePalette::Mode)));
        assert!(
            !std::iter::from_fn(|| commands.try_recv().ok())
                .any(|cmd| matches!(cmd, InlineCommand::ShowTransient { .. }))
        );

        *harness.palette_state = None;
        let outcome = handle_slash_command(command, &mut harness.renderer, workspace.path())
            .await
            .unwrap();
        route_outcome(outcome, harness.ctx()).await.unwrap();
        assert!(matches!(harness.palette_state(), Some(ActivePalette::Settings { state, .. })
            if state.view_path.as_deref() == Some(SECRETS_VIEW)));
        assert!(std::iter::from_fn(|| commands.try_recv().ok()).any(|cmd| matches!(cmd,
            InlineCommand::ShowTransient { request } if matches!(request.as_ref(), TransientRequest::List(list)
                if list.search.as_ref().is_some_and(|search| search.fuzzy)
                    && list.items.iter().any(|item| item.title == "Migrate .env keys")))));
    }
}

#[tokio::test]
#[serial_test::serial]
async fn secret_migration_and_deletion_refresh_the_active_credential() {
    use super::secrets::{clear_secret_core, migrate_env_core, resolve_secret_target};
    use vtcode_auth::AuthCredentialsStoreMode;
    use vtcode_commons::ui_protocol::InlineTone;
    use vtcode_config::api_keys::load_stored_credential_with_mode;

    const ENV_KEY: &str = "VTCODE_TEST_ACTIVE_PROVIDER_KEY";
    const MIGRATED_KEY: &str = "disposable-migration-fixture";
    const FALLBACK_KEY: &str = "disposable-workspace-fallback";
    let mut harness = HeadlessHarness::new().await;
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let env = vtcode_commons::env_lock::lock();
    let previous_config = std::env::var_os("VTCODE_CONFIG");
    let previous_key = std::env::var_os(ENV_KEY);
    env.set_var("VTCODE_CONFIG", workspace.path().join("storage"));
    env.remove_var(ENV_KEY);

    let ctx = harness.ctx();
    ctx.config.workspace = workspace.path().to_path_buf();
    ctx.config.provider = "gemini".to_string();
    ctx.config.api_key_env = ENV_KEY.to_string();
    let mut fixture = VTCodeConfig::default();
    fixture.agent.credential_storage_mode = AuthCredentialsStoreMode::File;
    fixture.custom_providers.push(vtcode_config::core::CustomProviderConfig {
        name: "migration-company".to_string(),
        display_name: "Migration Company".to_string(),
        base_url: "https://example.invalid/v1".to_string(),
        api_key_env: ENV_KEY.to_string(),
        ..Default::default()
    });
    let targets = [
        resolve_secret_target(Some(&fixture), "gemini", Some(ENV_KEY)).unwrap(),
        resolve_secret_target(Some(&fixture), "migration-company", None).unwrap(),
    ];
    let dotenv = workspace.path().join(".env");
    std::fs::write(&dotenv, format!("{ENV_KEY}={MIGRATED_KEY}\nPRESERVE_ME=hello\n")).unwrap();
    let status = migrate_env_core(&targets, ctx.config, Some(&fixture), ctx.provider_client);
    assert_eq!(status.tone, InlineTone::Success);
    assert!(status.message.contains("Migration Company") && !status.message.contains(MIGRATED_KEY));
    assert_eq!(std::fs::read_to_string(&dotenv).unwrap(), "PRESERVE_ME=hello\n");
    for provider in ["gemini", "migration-company"] {
        let stored = load_stored_credential_with_mode(provider, ENV_KEY, AuthCredentialsStoreMode::File).unwrap();
        assert!(stored.as_deref() == Some(MIGRATED_KEY), "migration failed storage read-back for {provider}");
    }
    assert!(ctx.config.api_key == MIGRATED_KEY, "migration retained the old runtime key");
    assert_eq!(ctx.provider_client.name(), "gemini");

    std::fs::write(&dotenv, format!("{ENV_KEY}={FALLBACK_KEY}\n")).unwrap();
    let status = clear_secret_core(&targets[0], "confirm", ctx.config, Some(&fixture), ctx.provider_client).unwrap();
    assert_eq!(status.tone, InlineTone::Success);
    assert!(status.message.contains("Workspace .env") && !status.message.contains(FALLBACK_KEY));
    assert!(ctx.config.api_key == FALLBACK_KEY, "deletion did not install the workspace fallback");
    assert!(
        load_stored_credential_with_mode("gemini", ENV_KEY, AuthCredentialsStoreMode::File)
            .unwrap()
            .is_none()
    );

    // If read-back or construction fails the store can succeed, but the
    // runtime must retain both the previous client and its matching key.
    ctx.config.provider = "migration-company".to_string();
    let previous_client = std::ptr::from_ref(ctx.provider_client.as_ref());
    let status =
        super::secrets::store_secret_core(&targets[1], MIGRATED_KEY, ctx.config, Some(&fixture), ctx.provider_client)
            .unwrap();
    assert_eq!(status.tone, InlineTone::Warning);
    assert!(status.message.contains("Restart"));
    assert!(ctx.config.api_key == FALLBACK_KEY);
    assert!(std::ptr::eq(previous_client, ctx.provider_client.as_ref()));

    env.restore_var(ENV_KEY, previous_key);
    env.restore_var("VTCODE_CONFIG", previous_config);
}
