//! Model-facing thread goal loop tools (`get_goal`, `update_goal`).
//!
//! Registered once per session after the tracker is restored, holding the
//! live tracker and the runloop emitter so model-driven mutations persist
//! and stream exactly like user-driven ones.

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
use crate::agent::runloop::unified::turn::session::slash_commands::SharedGoalTracker;

/// Model tool reading the thread goal loop state.
#[derive(Clone)]
pub struct GetGoalTool {
    tracker: SharedGoalTracker,
}

impl GetGoalTool {
    pub fn new(tracker: SharedGoalTracker) -> Self {
        Self { tracker }
    }
}

#[async_trait]
impl Tool for GetGoalTool {
    async fn execute(&self, _args: Value) -> Result<Value> {
        let tracker = self.tracker.lock().await;
        Ok(json!({ "goal": tracker.goal_snapshot() }))
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
    emitter: Option<HarnessEventEmitter>,
}

impl UpdateGoalTool {
    pub fn new(tracker: SharedGoalTracker, emitter: Option<HarnessEventEmitter>) -> Self {
        Self { tracker, emitter }
    }

    fn parse_args(args: &Value) -> Result<(&str, String)> {
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
        Ok((action, reason.to_string()))
    }
}

#[async_trait]
impl Tool for UpdateGoalTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let (action, reason) = Self::parse_args(&args)?;
        let mut tracker = self.tracker.lock().await;
        let applied = match action {
            "complete" => tracker.complete(),
            "block" => tracker.pause_with_message(vtcode_memory::GoalPauseReason::ModelBlocked, reason.clone()),
            _ => tracker.pause_with_message(vtcode_memory::GoalPauseReason::User, reason.clone()),
        };
        if applied {
            tracker.persist().context("goal snapshot persist failed")?;
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

/// Register the goal loop tools for one session. Called from session setup
/// after the tracker is restored; mirrors the skill-tool registration shape.
pub(crate) async fn register_goal_tools(
    tool_registry: &mut ToolRegistry,
    goal_tracker: SharedGoalTracker,
    emitter: Option<HarnessEventEmitter>,
) -> Result<()> {
    let get_tool = GetGoalTool::new(goal_tracker.clone());
    let get_reg = ToolRegistration::from_tool_instance(tool_constants::GET_GOAL, CapabilityLevel::Basic, get_tool)
        .with_catalog_source(ToolCatalogSource::Builtin)
        .with_native_cgp_factory(native_cgp_tool_factory({
            let goal_tracker = goal_tracker.clone();
            move || GetGoalTool::new(goal_tracker.clone())
        }));
    tool_registry
        .register_tool(get_reg)
        .await
        .context("Failed to register get_goal tool")?;

    let update_tool = UpdateGoalTool::new(goal_tracker.clone(), emitter.clone());
    let update_reg =
        ToolRegistration::from_tool_instance(tool_constants::UPDATE_GOAL, CapabilityLevel::Basic, update_tool)
            .with_catalog_source(ToolCatalogSource::Builtin)
            .with_native_cgp_factory(native_cgp_tool_factory({
                let goal_tracker = goal_tracker.clone();
                let emitter = emitter.clone();
                move || UpdateGoalTool::new(goal_tracker.clone(), emitter.clone())
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
        (dir, std::sync::Arc::new(tokio::sync::Mutex::new(tracker)))
    }

    #[tokio::test]
    async fn get_goal_reports_null_then_snapshot() {
        let (_dir, tracker) = test_tracker();
        let tool = GetGoalTool::new(tracker.clone());

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
        let full = tool.execute(json!({})).await.unwrap();
        assert_eq!(full["goal"]["objective"], "Ship it");
        assert_eq!(full["goal"]["state"], "active");
    }

    #[tokio::test]
    async fn update_goal_completes_blocks_and_pauses() {
        let (_dir, tracker) = test_tracker();
        let tool = UpdateGoalTool::new(tracker.clone(), None);

        let missing = tool.execute(json!({"action": "complete", "reason": "done"})).await.unwrap();
        assert_eq!(missing["applied"], false);
        assert!(missing["message"].as_str().unwrap().contains("no thread goal"));

        tracker.lock().await.create_goal(
            "goal-1".into(),
            "Ship it".into(),
            None,
            0,
            "2026-01-01T00:00:00Z".into(),
            None,
        );
        let blocked = tool
            .execute(json!({"action": "block", "reason": "waiting on keys"}))
            .await
            .unwrap();
        assert_eq!(blocked["applied"], true);
        assert_eq!(blocked["status"], "blocked");
        assert_eq!(tracker.lock().await.status().unwrap().display_name(), "Blocked");

        assert!(tracker.lock().await.resume());
        let paused = tool.execute(json!({"action": "pause", "reason": "user asked"})).await.unwrap();
        assert_eq!(paused["applied"], true);
        assert_eq!(paused["status"], "paused");

        assert!(tracker.lock().await.resume());
        let done = tool.execute(json!({"action": "complete", "reason": "verified"})).await.unwrap();
        assert_eq!(done["applied"], true);
        assert_eq!(done["status"], "complete");
    }

    #[tokio::test]
    async fn update_goal_rejects_bad_input() {
        let (_dir, tracker) = test_tracker();
        let tool = UpdateGoalTool::new(tracker, None);

        assert!(tool.execute(json!({"action": "explode", "reason": "x"})).await.is_err());
        assert!(tool.execute(json!({"action": "complete", "reason": "  "})).await.is_err());
        assert!(tool.execute(json!({"action": "complete"})).await.is_err());
        assert!(tool.validate_args(&json!({"action": "complete", "reason": "ok"})).is_ok());
    }
}
