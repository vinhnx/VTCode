//! Model-facing thread goal loop tools (`get_goal`, `update_goal`).
//!
//! Registered once per session after the tracker is restored, holding the
//! live tracker and the runloop emitter so model-driven mutations persist
//! and stream exactly like user-driven ones.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use vtcode_config::constants::tools as tool_constants;
use vtcode_core::config::types::CapabilityLevel;
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::native_cgp_tool_factory;
use vtcode_core::tools::registry::{ToolCatalogSource, ToolRegistration};
use vtcode_core::tools::traits::Tool;
use vtcode_utility_tool_specs::{
    get_goal_description, get_goal_parameters, update_goal_description, update_goal_parameters,
};

use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::stop_requests::{HostGoalHandles, StopHandleSlot, publish_goal_handles};
use crate::agent::runloop::unified::turn::session::slash_commands::SharedGoalTracker;

/// Model tool reading the thread goal loop state.
#[derive(Clone)]
pub struct GetGoalTool {
    tracker: SharedGoalTracker,
    token_total: Arc<AtomicI64>,
}

impl GetGoalTool {
    fn new(tracker: SharedGoalTracker, token_total: Arc<AtomicI64>) -> Self {
        Self { tracker, token_total }
    }
}

#[async_trait]
impl Tool for GetGoalTool {
    async fn execute(&self, _args: Value) -> Result<Value> {
        let mut tracker = self.tracker.lock().await;
        tracker.record_token_observation(self.token_total.load(Ordering::Relaxed));
        Ok(json!({ "goal": tracker.goal_snapshot(), "activation_id": tracker.active_activation_id() }))
    }

    fn name(&self) -> &str {
        tool_constants::GET_GOAL
    }

    fn description(&self) -> &str {
        get_goal_description()
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(get_goal_parameters())
    }

    fn default_permission(&self) -> ToolPolicy {
        ToolPolicy::Allow
    }

    fn is_mutating(&self) -> bool {
        false
    }

    fn is_parallel_safe(&self) -> bool {
        true
    }

    fn kind(&self) -> &'static str {
        "goal"
    }
}

/// Model tool completing, blocking, or pausing the thread goal loop.
#[derive(Clone)]
pub struct UpdateGoalTool {
    tracker: SharedGoalTracker,
    token_total: Arc<AtomicI64>,
    emitter: Option<HarnessEventEmitter>,
    verification_pending: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl UpdateGoalTool {
    fn new(
        tracker: SharedGoalTracker,
        token_total: Arc<AtomicI64>,
        emitter: Option<HarnessEventEmitter>,
        verification_pending: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        Self {
            tracker,
            token_total,
            emitter,
            verification_pending,
        }
    }

    fn parse_args(args: &Value) -> Result<(&str, String, uuid::Uuid)> {
        let action = args
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("update_goal requires an action"))?;
        if !matches!(action, "complete" | "block" | "pause") {
            bail!("unknown goal action: {action}");
        }
        let reason = args.get("reason").and_then(Value::as_str).unwrap_or("").trim();
        if reason.is_empty() {
            bail!("update_goal requires a non-empty reason");
        }
        let activation_id = args
            .get("activation_id")
            .and_then(Value::as_str)
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .context("update_goal requires the activation_id returned by get_goal")?;
        Ok((action, reason.to_string(), activation_id))
    }
}

#[async_trait]
impl Tool for UpdateGoalTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let (action, reason, activation_id) = Self::parse_args(&args)?;
        let mut tracker = self.tracker.lock().await;
        if tracker.active_activation_id() != Some(activation_id) {
            bail!("goal activation changed; read get_goal before updating");
        }
        if action == "complete" && (self.verification_pending)() {
            bail!("goal completion requires the pending verification checks to pass");
        }
        let mut staged = tracker.clone();
        staged.record_token_observation(self.token_total.load(Ordering::Relaxed));
        let applied = match action {
            "complete" => staged.complete(),
            "block" => staged.pause_with_message(vtcode_memory::GoalPauseReason::ModelBlocked, reason.clone()),
            _ => staged.pause_with_message(vtcode_memory::GoalPauseReason::User, reason.clone()),
        };
        if applied {
            tracker.commit(staged).context("goal snapshot persist failed")?;
            if let Some(emitter) = &self.emitter
                && let Err(err) = emitter.emit(tracker.to_goal_updated_event())
            {
                tracing::warn!("goal event emit failed: {err:#}");
            }
        }
        let snapshot = tracker.goal_snapshot();
        let message = if applied {
            match action {
                "complete" => "Goal marked complete.".to_string(),
                "block" => format!("Goal blocked: {reason}"),
                _ => format!("Goal paused: {reason}"),
            }
        } else if let Some(status) = tracker.status() {
            format!("No state change: goal is {}.", status.display_name())
        } else {
            "No state change: no thread goal is set.".to_string()
        };
        Ok(json!({
            "applied": applied,
            "status": snapshot.as_ref().map(|s| s.state.as_str()),
            "message": message,
            "goal": snapshot,
        }))
    }

    fn name(&self) -> &str {
        tool_constants::UPDATE_GOAL
    }

    fn description(&self) -> &str {
        update_goal_description()
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(update_goal_parameters())
    }

    fn validate_args(&self, args: &Value) -> Result<()> {
        Self::parse_args(args).map(|_| ())
    }

    fn default_permission(&self) -> ToolPolicy {
        // Session-local state only: visible in the goal bar and events, and
        // recoverable by re-setting. Prompting here would wedge headless and
        // desktop loops that complete autonomously.
        ToolPolicy::Allow
    }

    fn is_mutating(&self) -> bool {
        true
    }

    fn is_parallel_safe(&self) -> bool {
        false
    }

    fn kind(&self) -> &'static str {
        "goal"
    }
}

/// Restore the tracker, wire its model tools, and publish the same state to the host.
pub(crate) async fn prepare_goal_runtime(
    session_dir: PathBuf,
    session_stats: &mut SessionStats,
    tool_registry: &mut ToolRegistry,
    emitter: Option<HarnessEventEmitter>,
    stop_slot: Option<&StopHandleSlot>,
) -> Result<SharedGoalTracker> {
    let tracker = match vtcode_memory::GoalTracker::restore(session_dir.clone()) {
        Ok(tracker) => tracker,
        Err(err) => {
            tracing::warn!("goal snapshot restore failed, starting without a goal: {err:#}");
            vtcode_memory::GoalTracker::new(session_dir)
        }
    };
    if let Some(emitter) = &emitter
        && tracker.has_goal()
        && let Err(err) = emitter.emit(tracker.to_goal_updated_event())
    {
        tracing::warn!("restored goal event emit failed: {err:#}");
    }
    let tracker = Arc::new(tokio::sync::Mutex::new(tracker));
    let token_total = session_stats.share_goal_token_total();
    register_goal_tools(tool_registry, tracker.clone(), token_total.clone(), emitter.clone()).await?;
    if let Some(slot) = stop_slot {
        publish_goal_handles(slot, HostGoalHandles { tracker: tracker.clone(), token_total, emitter });
    }
    Ok(tracker)
}

async fn register_goal_tools(
    tool_registry: &mut ToolRegistry,
    goal_tracker: SharedGoalTracker,
    token_total: Arc<AtomicI64>,
    emitter: Option<HarnessEventEmitter>,
) -> Result<()> {
    let get_tool = GetGoalTool::new(goal_tracker.clone(), token_total.clone());
    let get_reg = ToolRegistration::from_tool_instance(tool_constants::GET_GOAL, CapabilityLevel::Basic, get_tool)
        .with_catalog_source(ToolCatalogSource::Builtin)
        .with_native_cgp_factory(native_cgp_tool_factory({
            let goal_tracker = goal_tracker.clone();
            let token_total = token_total.clone();
            move || GetGoalTool::new(goal_tracker.clone(), token_total.clone())
        }));
    tool_registry
        .register_tool(get_reg)
        .await
        .context("Failed to register get_goal tool")?;

    let verification_pending = Arc::new(tool_registry.completion_verification_check());
    let update_tool =
        UpdateGoalTool::new(goal_tracker.clone(), token_total.clone(), emitter.clone(), verification_pending.clone());
    let update_reg =
        ToolRegistration::from_tool_instance(tool_constants::UPDATE_GOAL, CapabilityLevel::Basic, update_tool)
            .with_catalog_source(ToolCatalogSource::Builtin)
            .with_native_cgp_factory(native_cgp_tool_factory({
                let goal_tracker = goal_tracker.clone();
                let emitter = emitter.clone();
                move || {
                    UpdateGoalTool::new(
                        goal_tracker.clone(),
                        token_total.clone(),
                        emitter.clone(),
                        verification_pending.clone(),
                    )
                }
            }));
    tool_registry
        .register_tool(update_reg)
        .await
        .context("Failed to register update_goal tool")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tracker() -> (tempfile::TempDir, SharedGoalTracker) {
        let dir = tempfile::tempdir().unwrap();
        let tracker = vtcode_memory::GoalTracker::new(dir.path().join("session-1"));
        (dir, Arc::new(tokio::sync::Mutex::new(tracker)))
    }

    #[tokio::test]
    async fn goal_runtime_shares_restored_state_between_host_and_registered_tools() {
        use crate::agent::runloop::unified::stop_requests::{host_goal_handles, new_stop_handle_slot};

        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("session-1");
        let mut prior = vtcode_memory::GoalTracker::new(session_dir.clone());
        prior.create_goal("prior-goal".into(), "Ship it".into(), None, 50, "2026-01-01T00:00:00Z".into(), None);
        prior.record_token_observation(67);
        prior.persist().unwrap();

        let mut registry = ToolRegistry::new_for_schema(dir.path().to_path_buf()).await;
        let mut stats = SessionStats::default();
        let slot = new_stop_handle_slot();
        let events = dir.path().join("restored-goal.jsonl");
        let emitter = HarnessEventEmitter::new(events.clone()).unwrap();
        let tracker = prepare_goal_runtime(session_dir, &mut stats, &mut registry, Some(emitter), Some(&slot))
            .await
            .unwrap();
        let host = host_goal_handles(&slot).unwrap();
        assert!(Arc::ptr_eq(&host.tracker, &tracker));
        assert!(registry.get_tool(tool_constants::UPDATE_GOAL).is_some());
        let get = registry.get_tool(tool_constants::GET_GOAL).unwrap();
        let result = get.execute(json!({})).await.unwrap();
        assert_eq!(result["goal"]["goal_id"], "prior-goal");
        assert_eq!(result["goal"]["state"], "paused");
        assert_eq!(result["goal"]["tokens_used"], 17);
        assert_eq!(host.token_total.load(Ordering::Relaxed), 0);
        let event = std::fs::read_to_string(events).unwrap();
        assert!(event.contains("\"goal.updated\"") && event.contains("\"paused\""), "{event}");
    }

    #[tokio::test]
    async fn get_goal_reports_null_then_snapshot() {
        let (_dir, tracker) = test_tracker();
        let total = Arc::new(AtomicI64::new(0));
        let tool = GetGoalTool::new(tracker.clone(), total.clone());

        let empty = tool.execute(json!({})).await.unwrap();
        assert!(empty["goal"].is_null(), "{empty}");

        tracker.lock().await.create_goal(
            "goal-1".into(),
            "Ship it".into(),
            None,
            0,
            "2026-01-01T00:00:00Z".into(),
            None,
        );
        total.store(240, Ordering::Relaxed);
        let full = tool.execute(json!({})).await.unwrap();
        assert_eq!(full["goal"]["objective"], "Ship it");
        assert_eq!(full["goal"]["state"], "active");
        assert_eq!(full["goal"]["tokens_used"], 240);
    }

    #[tokio::test]
    async fn update_goal_completes_blocks_and_pauses() {
        let (_dir, tracker) = test_tracker();
        let total = Arc::new(AtomicI64::new(0));
        let pending = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let tool = UpdateGoalTool::new(
            tracker.clone(),
            total.clone(),
            None,
            Arc::new({
                let pending = pending.clone();
                move || pending.load(Ordering::Relaxed)
            }),
        );

        assert!(
            tool.execute(json!({"action": "complete", "reason": "done", "activation_id": uuid::Uuid::nil()}))
                .await
                .is_err()
        );

        tracker.lock().await.create_goal(
            "goal-1".into(),
            "Ship it".into(),
            None,
            0,
            "2026-01-01T00:00:00Z".into(),
            None,
        );
        let old_id = tracker.lock().await.active_activation_id().unwrap();
        let blocked = tool
            .execute(json!({"action": "block", "reason": "waiting on keys", "activation_id": old_id}))
            .await
            .unwrap();
        assert_eq!(blocked["applied"], true);
        assert_eq!(blocked["status"], "blocked");
        assert_eq!(tracker.lock().await.status().unwrap().display_name(), "Blocked");

        assert!(tracker.lock().await.resume());
        let resumed_id = tracker.lock().await.active_activation_id().unwrap();
        assert!(
            tool.execute(json!({"action": "complete", "reason": "stale", "activation_id": old_id}))
                .await
                .is_err()
        );
        let paused = tool
            .execute(json!({"action": "pause", "reason": "user asked", "activation_id": resumed_id}))
            .await
            .unwrap();
        assert_eq!(paused["applied"], true);
        assert_eq!(paused["status"], "paused");

        assert!(tracker.lock().await.resume());
        total.store(137, Ordering::Relaxed);
        let active = tracker.lock().await.active_activation_id().unwrap();
        let action = json!({"action": "complete", "reason": "verified", "activation_id": active});
        pending.store(true, Ordering::Relaxed);
        assert!(
            tool.execute(action.clone())
                .await
                .unwrap_err()
                .to_string()
                .contains("verification")
        );
        assert!(tracker.lock().await.is_active());
        pending.store(false, Ordering::Relaxed);
        let done = tool.execute(action).await.unwrap();
        assert_eq!(done["applied"], true);
        assert_eq!(done["status"], "complete");
        assert_eq!(done["goal"]["tokens_used"], 137);
        total.store(480, Ordering::Relaxed);
        let after = GetGoalTool::new(tracker, total).execute(json!({})).await.unwrap();
        assert_eq!(after["goal"]["tokens_used"], 137);
    }

    #[tokio::test]
    async fn update_goal_rejects_bad_input() {
        let (_dir, tracker) = test_tracker();
        let tool = UpdateGoalTool::new(tracker, Arc::default(), None, Arc::new(|| false));

        assert!(tool.execute(json!({"action": "explode", "reason": "x"})).await.is_err());
        assert!(tool.execute(json!({"action": "complete", "reason": "  "})).await.is_err());
        assert!(tool.execute(json!({"action": "complete"})).await.is_err());
        assert!(
            tool.validate_args(&json!({"action": "complete", "reason": "ok", "activation_id": uuid::Uuid::nil()}))
                .is_ok()
        );
    }
}
