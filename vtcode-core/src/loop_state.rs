//! Loop run state persistence for loop-engineering workflows.
//!
//! A loop is a long-lived scheduler that invokes the vtcode harness repeatedly.
//! `LoopRunState` captures the durable state a loop scheduler reads on resume:
//! current step index, cumulative token cost, last artifact path, and status.
//!
//! State is persisted as JSON under `{workspace}/.vtcode/state/loop-{id}.json`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static WRITE_COUNTER: AtomicU64 = AtomicU64::new(1);

const STATE_DIR_NAME: &str = "state";

// ─── Loop Run State ──────────────────────────────────────────────────────────

/// Durable state for a single loop run. The loop scheduler reads this on
/// resume to know where execution left off.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopRunState {
    /// Unique identifier for this loop run.
    pub loop_id: String,
    /// Zero-based index of the current step.
    pub step_index: u32,
    /// Cumulative token usage across all steps so far.
    pub cumulative_tokens: TokenUsage,
    /// Path to the last artifact produced by the loop (e.g., a diff, a report).
    pub last_artifact_path: Option<PathBuf>,
    /// Current lifecycle status.
    pub status: LoopStatus,
    /// When the loop run started.
    pub started_at: DateTime<Utc>,
    /// When the loop state was last persisted.
    pub updated_at: DateTime<Utc>,
    /// When the last LLM request was sent. Used to detect cache expiration
    /// after long pauses (the article flags this as a silent cost driver).
    #[serde(default)]
    pub last_request_at: Option<DateTime<Utc>>,
}

/// Lifecycle status of a loop run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus {
    /// The loop is actively running.
    Running,
    /// The loop completed all steps successfully.
    Completed,
    /// The loop failed and cannot resume.
    Failed,
    /// The loop was paused and can be resumed.
    Paused,
}

impl LoopRunState {
    /// Create a new loop run state with the given identifier.
    pub fn new(loop_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            loop_id: loop_id.into(),
            step_index: 0,
            cumulative_tokens: TokenUsage::default(),
            last_artifact_path: None,
            status: LoopStatus::Running,
            started_at: now,
            updated_at: now,
            last_request_at: None,
        }
    }

    /// Advance to the next step and update the timestamp.
    pub fn advance_step(&mut self) {
        self.step_index = self.step_index.saturating_add(1);
        self.updated_at = Utc::now();
    }

    /// Record token usage from a completed step.
    pub fn record_usage(&mut self, usage: &TokenUsage) {
        self.cumulative_tokens.input_tokens = self
            .cumulative_tokens
            .input_tokens
            .saturating_add(usage.input_tokens);
        self.cumulative_tokens.output_tokens = self
            .cumulative_tokens
            .output_tokens
            .saturating_add(usage.output_tokens);
        self.cumulative_tokens.cached_input_tokens = self
            .cumulative_tokens
            .cached_input_tokens
            .saturating_add(usage.cached_input_tokens);
        self.cumulative_tokens.cache_creation_tokens = self
            .cumulative_tokens
            .cache_creation_tokens
            .saturating_add(usage.cache_creation_tokens);
        // Guard against NaN: if either operand is NaN, the addition produces NaN,
        // and NaN >= max_budget_usd evaluates to false, allowing an unbounded loop.
        // Clamp to finite values to ensure the budget check always works.
        let new_cost = self.cumulative_tokens.total_cost_usd + usage.total_cost_usd;
        self.cumulative_tokens.total_cost_usd = if new_cost.is_finite() {
            new_cost
        } else {
            f64::MAX
        };
        self.updated_at = Utc::now();
    }

    /// Mark the loop as completed.
    pub fn mark_completed(&mut self) {
        self.status = LoopStatus::Completed;
        self.updated_at = Utc::now();
    }

    /// Mark the loop as failed.
    pub fn mark_failed(&mut self) {
        self.status = LoopStatus::Failed;
        self.updated_at = Utc::now();
    }

    /// Mark the loop as paused.
    pub fn mark_paused(&mut self) {
        self.status = LoopStatus::Paused;
        self.updated_at = Utc::now();
    }

    /// Returns true if the loop can be resumed.
    pub fn is_resumable(&self) -> bool {
        matches!(self.status, LoopStatus::Paused | LoopStatus::Running)
    }

    /// Record that an LLM request was just sent. Called before each provider
    /// request so cache-gap detection can measure the pause since the last one.
    pub fn note_request_sent(&mut self) {
        self.last_request_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Check whether the cache may have expired since the last request.
    ///
    /// Returns `Some(duration)` if the gap exceeds `threshold_secs`, indicating
    /// the next request will likely incur full cache creation cost. The article
    /// identifies this as a silent cost driver: "resuming a session after a long
    /// pause with its cache expired."
    #[must_use]
    pub fn cache_gap_exceeds(&self, threshold_secs: i64) -> Option<chrono::TimeDelta> {
        let last = self.last_request_at?;
        let elapsed = Utc::now() - last;
        if elapsed.num_seconds() >= threshold_secs {
            Some(elapsed)
        } else {
            None
        }
    }

    /// Produce a human-readable summary of cache efficiency for this session.
    #[must_use]
    pub fn cache_summary(&self) -> String {
        let t = &self.cumulative_tokens;
        let total_input = t.input_tokens;
        let cached = t.cached_input_tokens;
        let creation = t.cache_creation_tokens;
        let uncached = t.uncached_input_tokens();

        if total_input == 0 {
            return "No input tokens recorded.".to_string();
        }

        let rate = cached as f64 / total_input as f64 * 100.0;
        format!(
            "Cache: {cached} cached / {total_input} total input ({rate:.1}% hit rate), \
             {creation} cache-creation, {uncached} uncached"
        )
    }
}

// ─── Token Usage ─────────────────────────────────────────────────────────────

/// Cumulative token usage for a loop run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Number of input tokens consumed.
    pub input_tokens: u64,
    /// Number of output tokens produced.
    pub output_tokens: u64,
    /// Estimated total cost in USD.
    pub total_cost_usd: f64,
    /// Number of input tokens served from cache (cache hits).
    /// These tokens cost significantly less than uncached tokens.
    #[serde(default)]
    pub cached_input_tokens: u64,
    /// Number of input tokens that created new cache entries.
    #[serde(default)]
    pub cache_creation_tokens: u64,
}

impl TokenUsage {
    /// Total tokens (input + output).
    #[must_use]
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    /// Number of input tokens that were NOT served from cache.
    /// These are the expensive tokens that drive most of the input cost.
    #[must_use]
    pub fn uncached_input_tokens(&self) -> u64 {
        self.input_tokens.saturating_sub(self.cached_input_tokens)
    }

    /// Cache hit rate as a fraction (0.0 to 1.0).
    /// Returns None if there are no input tokens.
    #[must_use]
    pub fn cache_hit_rate(&self) -> Option<f64> {
        if self.input_tokens == 0 {
            return None;
        }
        Some(self.cached_input_tokens as f64 / self.input_tokens as f64)
    }

    /// Cache-aware effective cost in USD.
    ///
    /// Cached tokens cost ~10% of uncached tokens (matching OpenAI's 10x
    /// pricing differential). This gives a more accurate cost estimate than
    /// treating all input tokens at the same rate.
    ///
    /// The formula: effective_cost = (uncached_tokens * base_rate)
    ///                              + (cached_tokens * base_rate * 0.1)
    ///                              + (cache_creation_tokens * base_rate * 0.25)
    ///                              + (output_tokens * output_rate)
    ///
    /// Recomputes cost from token counts and per-token rates rather than
    /// adjusting `total_cost_usd`, since we need granular cache discounts.
    #[must_use]
    pub fn cache_adjusted_cost_usd(&self, base_input_rate: f64, output_rate: f64) -> f64 {
        let uncached = self.uncached_input_tokens() as f64;
        let cached = self.cached_input_tokens as f64;
        let creation = self.cache_creation_tokens as f64;
        let output = self.output_tokens as f64;

        // Cached tokens cost 10% of base rate, creation costs 25% premium
        (uncached * base_input_rate)
            + (cached * base_input_rate * 0.1)
            + (creation * base_input_rate * 0.25)
            + (output * output_rate)
    }
}

// ─── Cost Budget ─────────────────────────────────────────────────────────────

/// Budget constraints for a loop run. Checked before each step to prevent
/// unbounded iteration cost — the primary failure mode Osmani identifies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBudget {
    /// Maximum total tokens (input + output) before the loop stops.
    pub max_total_tokens: u64,
    /// Maximum total cost in USD before the loop stops.
    pub max_cost_usd: f64,
    /// Maximum number of steps before the loop stops.
    pub max_steps: u32,
}

impl Default for CostBudget {
    fn default() -> Self {
        Self {
            max_total_tokens: 1_000_000,
            max_cost_usd: 10.0,
            max_steps: 50,
        }
    }
}

/// Result of checking a loop run state against a budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetStatus {
    /// The loop can continue.
    Ok,
    /// The token limit has been reached.
    TokenLimitReached,
    /// The cost limit has been reached.
    CostLimitReached,
    /// The step limit has been reached.
    StepLimitReached,
}

impl CostBudget {
    /// Check whether the loop run state is within budget.
    #[must_use]
    pub fn check(&self, state: &LoopRunState) -> BudgetStatus {
        if state.step_index >= self.max_steps {
            return BudgetStatus::StepLimitReached;
        }
        if state.cumulative_tokens.total_tokens() >= self.max_total_tokens {
            return BudgetStatus::TokenLimitReached;
        }
        if state.cumulative_tokens.total_cost_usd >= self.max_cost_usd {
            return BudgetStatus::CostLimitReached;
        }
        BudgetStatus::Ok
    }
}

// ─── Persistence ─────────────────────────────────────────────────────────────

/// Resolve the `.vtcode/state/` directory for a workspace.
pub fn state_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".vtcode").join(STATE_DIR_NAME)
}

/// Resolve the path for a specific loop state file.
pub fn loop_state_path(workspace_root: &Path, loop_id: &str) -> PathBuf {
    state_dir(workspace_root).join(format!("loop-{loop_id}.json"))
}

/// Save loop run state to disk using atomic write (temp file + rename).
pub fn save_loop_state(workspace_root: &Path, state: &LoopRunState) -> Result<PathBuf> {
    let dir = state_dir(workspace_root);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create state directory {}", dir.display()))?;

    let path = loop_state_path(workspace_root, &state.loop_id);
    let serialized =
        serde_json::to_vec_pretty(state).context("Failed to serialize loop run state")?;

    atomic_write(&path, &serialized)?;
    Ok(path)
}

/// Load loop run state from disk.
pub fn load_loop_state(workspace_root: &Path, loop_id: &str) -> Result<Option<LoopRunState>> {
    let path = loop_state_path(workspace_root, loop_id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read loop state {}", path.display()))?;
    let state: LoopRunState = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(state))
}

/// Delete a loop state file from disk.
pub fn delete_loop_state(workspace_root: &Path, loop_id: &str) -> Result<bool> {
    let path = loop_state_path(workspace_root, loop_id);
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("Failed to delete loop state {}", path.display()))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// List all loop state files in the state directory.
pub fn list_loop_states(workspace_root: &Path) -> Result<Vec<LoopRunState>> {
    let dir = state_dir(workspace_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut states = Vec::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("Failed to read state directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("loop-") && n.ends_with(".json"))
        {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            match serde_json::from_str::<LoopRunState>(&raw) {
                Ok(state) => states.push(state),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "Skipping malformed loop state file");
                    continue;
                }
            }
        }
    }
    states.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(states)
}

/// Atomic write using temp file + rename, matching the pattern from
/// `scheduler/mod.rs:1381`.
fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let temp_name = format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("loop-state"),
        WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let temp_path = path.with_file_name(temp_name);
    fs::write(&temp_path, content)
        .with_context(|| format!("Failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, path)
        .with_context(|| format!("Failed to replace {}", path.display()))?;
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn loop_run_state_new_has_correct_defaults() {
        let state = LoopRunState::new("test-loop");
        assert_eq!(state.loop_id, "test-loop");
        assert_eq!(state.step_index, 0);
        assert_eq!(state.status, LoopStatus::Running);
        assert!(state.last_artifact_path.is_none());
        assert_eq!(state.cumulative_tokens.input_tokens, 0);
        assert_eq!(state.cumulative_tokens.output_tokens, 0);
    }

    #[test]
    fn loop_run_state_advance_step_increments() {
        let mut state = LoopRunState::new("test");
        assert_eq!(state.step_index, 0);
        state.advance_step();
        assert_eq!(state.step_index, 1);
        state.advance_step();
        assert_eq!(state.step_index, 2);
    }

    #[test]
    fn loop_run_state_record_usage_accumulates() {
        let mut state = LoopRunState::new("test");
        state.record_usage(&TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_cost_usd: 0.01,
            ..Default::default()
        });
        assert_eq!(state.cumulative_tokens.input_tokens, 100);
        assert_eq!(state.cumulative_tokens.output_tokens, 50);
        assert!((state.cumulative_tokens.total_cost_usd - 0.01).abs() < f64::EPSILON);

        state.record_usage(&TokenUsage {
            input_tokens: 200,
            output_tokens: 100,
            total_cost_usd: 0.02,
            ..Default::default()
        });
        assert_eq!(state.cumulative_tokens.input_tokens, 300);
        assert_eq!(state.cumulative_tokens.output_tokens, 150);
        assert!((state.cumulative_tokens.total_cost_usd - 0.03).abs() < f64::EPSILON);
    }

    #[test]
    fn loop_run_state_status_transitions() {
        let mut state = LoopRunState::new("test");
        assert_eq!(state.status, LoopStatus::Running);
        assert!(state.is_resumable());

        state.mark_paused();
        assert_eq!(state.status, LoopStatus::Paused);
        assert!(state.is_resumable());

        state.mark_completed();
        assert_eq!(state.status, LoopStatus::Completed);
        assert!(!state.is_resumable());

        let mut state2 = LoopRunState::new("test2");
        state2.mark_failed();
        assert_eq!(state2.status, LoopStatus::Failed);
        assert!(!state2.is_resumable());
    }

    #[test]
    fn token_usage_total_tokens() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_cost_usd: 0.0,
            ..Default::default()
        };
        assert_eq!(usage.total_tokens(), 150);
    }

    #[test]
    fn cost_budget_ok_within_limits() {
        let budget = CostBudget {
            max_total_tokens: 1000,
            max_cost_usd: 1.0,
            max_steps: 10,
        };
        let state = LoopRunState::new("test");
        assert_eq!(budget.check(&state), BudgetStatus::Ok);
    }

    #[test]
    fn cost_budget_step_limit_reached() {
        let budget = CostBudget {
            max_total_tokens: 1_000_000,
            max_cost_usd: 100.0,
            max_steps: 3,
        };
        let mut state = LoopRunState::new("test");
        state.advance_step(); // 1
        assert_eq!(budget.check(&state), BudgetStatus::Ok);
        state.advance_step(); // 2
        assert_eq!(budget.check(&state), BudgetStatus::Ok);
        state.advance_step(); // 3
        assert_eq!(budget.check(&state), BudgetStatus::StepLimitReached);
    }

    #[test]
    fn cost_budget_token_limit_reached() {
        let budget = CostBudget {
            max_total_tokens: 500,
            max_cost_usd: 100.0,
            max_steps: 100,
        };
        let mut state = LoopRunState::new("test");
        state.record_usage(&TokenUsage {
            input_tokens: 300,
            output_tokens: 250,
            total_cost_usd: 0.0,
            ..Default::default()
        });
        assert_eq!(budget.check(&state), BudgetStatus::TokenLimitReached);
    }

    #[test]
    fn cost_budget_cost_limit_reached() {
        let budget = CostBudget {
            max_total_tokens: 1_000_000,
            max_cost_usd: 0.05,
            max_steps: 100,
        };
        let mut state = LoopRunState::new("test");
        state.record_usage(&TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_cost_usd: 0.06,
            ..Default::default()
        });
        assert_eq!(budget.check(&state), BudgetStatus::CostLimitReached);
    }

    #[test]
    fn loop_state_round_trip_persistence() {
        let tmp = TempDir::new().expect("temp dir");
        let mut state = LoopRunState::new("round-trip-test");
        state.advance_step();
        state.record_usage(&TokenUsage {
            input_tokens: 500,
            output_tokens: 200,
            total_cost_usd: 0.05,
            ..Default::default()
        });
        state.last_artifact_path = Some(PathBuf::from("/tmp/artifact.txt"));

        let path = save_loop_state(tmp.path(), &state).expect("save");
        assert!(path.exists());

        let loaded = load_loop_state(tmp.path(), "round-trip-test")
            .expect("load")
            .expect("should exist");
        assert_eq!(loaded.loop_id, "round-trip-test");
        assert_eq!(loaded.step_index, 1);
        assert_eq!(loaded.cumulative_tokens.input_tokens, 500);
        assert_eq!(loaded.cumulative_tokens.output_tokens, 200);
        assert!(loaded.last_artifact_path.is_some());
        assert_eq!(loaded.status, LoopStatus::Running);
    }

    #[test]
    fn load_loop_state_returns_none_for_missing() {
        let tmp = TempDir::new().expect("temp dir");
        let result = load_loop_state(tmp.path(), "nonexistent").expect("ok");
        assert!(result.is_none());
    }

    #[test]
    fn delete_loop_state_removes_file() {
        let tmp = TempDir::new().expect("temp dir");
        let state = LoopRunState::new("delete-me");
        save_loop_state(tmp.path(), &state).expect("save");

        let deleted = delete_loop_state(tmp.path(), "delete-me").expect("delete");
        assert!(deleted);

        let loaded = load_loop_state(tmp.path(), "delete-me").expect("load");
        assert!(loaded.is_none());
    }

    #[test]
    fn list_loop_states_returns_sorted_by_updated_at() {
        let tmp = TempDir::new().expect("temp dir");

        let mut state1 = LoopRunState::new("loop-1");
        state1.updated_at = Utc::now() - chrono::Duration::hours(1);
        save_loop_state(tmp.path(), &state1).expect("save");

        let mut state2 = LoopRunState::new("loop-2");
        state2.updated_at = Utc::now();
        save_loop_state(tmp.path(), &state2).expect("save");

        let states = list_loop_states(tmp.path()).expect("list");
        assert_eq!(states.len(), 2);
        // Most recent first
        assert_eq!(states[0].loop_id, "loop-2");
        assert_eq!(states[1].loop_id, "loop-1");
    }

    #[test]
    fn loop_state_serializes_status_variants() {
        for status in [
            LoopStatus::Running,
            LoopStatus::Completed,
            LoopStatus::Failed,
            LoopStatus::Paused,
        ] {
            let state = LoopRunState {
                loop_id: "serde-test".to_string(),
                step_index: 0,
                cumulative_tokens: TokenUsage::default(),
                last_artifact_path: None,
                status: status.clone(),
                started_at: Utc::now(),
                updated_at: Utc::now(),
                last_request_at: None,
            };
            let json = serde_json::to_string(&state).expect("serialize");
            let deserialized: LoopRunState = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(deserialized.status, status);
        }
    }
}
