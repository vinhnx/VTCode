//! Durable, compaction-safe progress ledger.
//!
//! Long-horizon agent capability requires a persistent signal of *goal
//! progress* that survives compaction, fork, and resume. The live conversation
//! is never reloaded into context from disk, but the progress ledger is a tiny
//! derived artifact (like `manifest.json`) that the harness can read on each
//! turn to decide whether work is actually advancing toward completion.
//!
//! The ledger is stored under `<session_dir>/derived/progress.json` and
//! overwritten on each update — it is a single mutable summary, not an
//! append-only log, which keeps reads O(1) and cheap.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::error::SessionStoreError;
use crate::session_dir;

/// Lifecycle status of a single milestone toward the session goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MilestoneStatus {
    /// Not yet started.
    Pending,
    /// Actively being worked on this/last turn.
    InProgress,
    /// Completed and verified.
    Done,
    /// Blocked — cannot proceed without external input or a replan.
    Blocked,
}

impl MilestoneStatus {
    /// Whether this status counts as forward progress toward completion.
    #[must_use]
    fn is_terminal(&self) -> bool {
        matches!(self, MilestoneStatus::Done)
    }
}

/// A single tracked milestone derived from the task tracker / plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Milestone {
    /// Stable identifier (e.g. tracker item index or plan item id).
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// Current status.
    pub status: MilestoneStatus,
}

/// Compact, durable progress signal for one session.
///
/// This is the harness's externalized memory of "are we getting closer to
/// done?" It is intentionally small so it can be loaded every turn without
/// touching the event log. Includes handoff metadata so cross-session
/// continuity is explicit in the ledger itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressLedger {
    /// Owning session id.
    pub session_id: String,
    /// The objective the agent is pursuing.
    pub goal: String,
    /// Tracked milestones; empty when the agent has no explicit tracker.
    milestones: Vec<Milestone>,
    /// Agent's confidence in eventual completion, `0.0..=1.0`.
    pub confidence: f32,
    /// RFC3339 timestamp of the first turn where no forward progress was
    /// detected, or `None` if progress is currently being made.
    stalled_since: Option<String>,
    /// RFC3339 timestamp of the last ledger update.
    updated_at: String,
    /// The session id of the predecessor session that handed off to this one.
    /// `None` for the first session in a chain.
    #[serde(default)]
    previous_session_id: Option<String>,
    /// Summary communicated by the previous session at handoff time.
    #[serde(default)]
    handoff_summary: Option<String>,
    /// Issues carried forward from the previous session.
    #[serde(default)]
    known_issues: Vec<String>,
    /// Git commit hash at the time of handoff (the "checkpoint").
    #[serde(default)]
    git_checkpoint: Option<String>,
}

impl ProgressLedger {
    /// Create a fresh ledger for a session with an initial goal.
    #[must_use]
    pub fn new(session_id: &str, goal: &str) -> Self {
        let ts = Utc::now().to_rfc3339();
        Self {
            session_id: session_id.to_string(),
            goal: goal.to_string(),
            milestones: Vec::new(),
            confidence: 1.0,
            stalled_since: None,
            updated_at: ts,
            previous_session_id: None,
            handoff_summary: None,
            known_issues: Vec::new(),
            git_checkpoint: None,
        }
    }

    /// Fraction of milestones in a terminal (`Done`) state, `0.0..=1.0`.
    /// Returns `1.0` when there are no milestones (nothing tracked yet).
    #[must_use]
    pub fn completion_ratio(&self) -> f32 {
        if self.milestones.is_empty() {
            return 1.0;
        }
        let done = self.milestones.iter().filter(|m| m.status.is_terminal()).count() as f32;
        done / self.milestones.len() as f32
    }

    /// Whether every tracked milestone is complete (or none are tracked).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.completion_ratio() >= 1.0
    }

    /// Whether the ledger currently reports a stall.
    #[must_use]
    pub fn is_stalled(&self) -> bool {
        self.stalled_since.is_some()
    }

    /// Record forward progress: clears any stall marker and refreshes the
    /// timestamp. Confidence is nudged upward (bounded at 1.0).
    pub fn note_advance(&mut self) {
        self.stalled_since = None;
        self.confidence = (self.confidence + 0.05).min(1.0);
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Record a stall: sets `stalled_since` on first occurrence and refreshes
    /// the timestamp. Confidence is nudged downward (bounded at `0.0`).
    pub fn note_stall(&mut self) {
        if self.stalled_since.is_none() {
            self.stalled_since = Some(Utc::now().to_rfc3339());
        }
        self.confidence = (self.confidence - 0.1).max(0.0);
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Replace the milestone set and refresh the timestamp.
    pub fn set_milestones(&mut self, milestones: Vec<Milestone>) {
        self.milestones = milestones;
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Set the session goal and refresh the timestamp.
    pub fn set_goal(&mut self, goal: &str) {
        self.goal = goal.to_string();
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Record handoff metadata from a previous session.
    fn set_handoff(&mut self, previous_session_id: &str, summary: &str, git_checkpoint: Option<String>) {
        self.previous_session_id = Some(previous_session_id.to_string());
        self.handoff_summary = Some(summary.to_string());
        self.git_checkpoint = git_checkpoint;
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Add a known issue carried forward from a previous session.
    fn add_known_issue(&mut self, issue: &str) {
        self.known_issues.push(issue.to_string());
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Render a compact, human-readable progress summary for durable memory
    /// (e.g. `<workspace>/memories/progress.md`). Survives compaction and gives
    /// a resumed session an accurate picture of what is done. Includes handoff
    /// metadata when present so the next session can orient from this alone.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Session Progress\n\n");
        let _ = writeln!(out, "**Goal:** {}", self.goal);
        let _ = writeln!(out, "**Completion:** {:.0}%", (self.completion_ratio() * 100.0).round());
        let _ = writeln!(out, "**Confidence:** {:.2}", self.confidence);
        if let Some(since) = &self.stalled_since {
            let _ = writeln!(out, "**Stalled since:** {since}");
        }
        let _ = writeln!(out, "**Updated:** {}\n", self.updated_at);

        if let Some(prev) = &self.previous_session_id {
            let _ = writeln!(out, "**Handed off from:** {prev}");
        }
        if let Some(summary) = &self.handoff_summary {
            let _ = writeln!(out, "**Handoff summary:** {summary}");
        }
        if let Some(checkpoint) = &self.git_checkpoint {
            let _ = writeln!(out, "**Git checkpoint:** `{checkpoint}`");
        }
        if !self.known_issues.is_empty() {
            out.push_str("\n## Known Issues\n\n");
            for issue in &self.known_issues {
                let _ = writeln!(out, "- {issue}");
            }
        }

        if self.milestones.is_empty() {
            out.push_str("\n_No tracked milestones yet._\n");
        } else {
            out.push_str("\n## Milestones\n\n");
            for m in &self.milestones {
                let mark = match m.status {
                    MilestoneStatus::Done => "[x]",
                    MilestoneStatus::InProgress => "[~]",
                    MilestoneStatus::Blocked => "[!]",
                    MilestoneStatus::Pending => "[ ]",
                };
                let _ = writeln!(out, "{} {} — {}", mark, m.id, m.description);
            }
        }
        out
    }
}

/// Resolve the on-disk path of the progress ledger for a session.
#[must_use]
pub fn progress_path(workspace: &Path, session_id: &str) -> PathBuf {
    session_dir(workspace, session_id)
        .join(crate::DERIVED_DIR)
        .join("progress.json")
}

/// Load the progress ledger for a session, if one has been persisted.
///
/// Returns `Ok(None)` when no ledger file exists yet (a fresh or pre-ledger
/// session) rather than an error, so callers can treat absence as "no signal".
pub fn load_progress(workspace: &Path, session_id: &str) -> Result<Option<ProgressLedger>, SessionStoreError> {
    let path = progress_path(workspace, session_id);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| SessionStoreError::io(path.clone(), e))?;
    let ledger: ProgressLedger = serde_json::from_slice(&bytes)?;
    Ok(Some(ledger))
}

/// Persist the progress ledger for a session, creating `derived/` if needed.
pub fn save_progress(workspace: &Path, session_id: &str, ledger: &ProgressLedger) -> Result<(), SessionStoreError> {
    let path = progress_path(workspace, session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| SessionStoreError::CreateDir { path: parent.to_path_buf(), source: e })?;
    }
    let bytes = serde_json::to_vec(ledger)?;
    std::fs::write(&path, bytes).map_err(|e| SessionStoreError::io(path, e))?;
    Ok(())
}

// ============================================================================
// Goal tracker state machine
// ============================================================================

use std::time::Instant;

/// Phase of goal execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalPhase {
    /// Goal is idle; no active planning or execution.
    Idle,
    /// Planning phase is in progress.
    Planning,
    /// Execution phase is in progress.
    Executing,
}

/// Lifecycle status of a goal.
///
/// The paused variants encode the reason the goal was paused:
/// - `UserPaused` for explicit pause requests
/// - `BackOffPaused` when the classifier run cap is hit
/// - `NoProgressPaused` when the verifier flags the same gaps with no progress
/// - `InfraPaused` when a turn finishes with an infrastructure error
/// - `Blocked` when the model determined the goal is not achievable
///
/// **Backwards-compat serde aliases:** older shells serialized this
/// enum with the default PascalCase form (`"Active"`, `"Paused"`,
/// `"BudgetLimited"`, `"Complete"`). The `#[serde(alias = ...)]`
/// attributes preserve in-flight goal snapshots written by older shells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    /// Goal is actively running.
    #[serde(alias = "Active")]
    Active,
    /// Explicitly paused by the user.
    #[serde(alias = "Paused")]
    UserPaused,
    /// Paused due to repeated classifier failures (back-off cap hit).
    BackOffPaused,
    /// Paused because the verifier reported the same gaps with no progress.
    NoProgressPaused,
    /// Paused due to an infrastructure error in a turn.
    InfraPaused,
    /// Blocked; the model determined the goal is not achievable.
    Blocked,
    /// Hit the token budget limit.
    #[serde(alias = "BudgetLimited")]
    BudgetLimited,
    /// Goal completed successfully.
    #[serde(alias = "Complete")]
    Complete,
}

impl<'de> Deserialize<'de> for GoalStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from_wire_str(&s))
    }
}

impl GoalStatus {
    /// Parse a persisted/wire status string. Unknown values map to
    /// `UserPaused`: a status this shell cannot interpret must restore as
    /// a resumable paused goal, never an Active self-driving one.
    fn from_wire_str(s: &str) -> Self {
        match s {
            "active" | "Active" => Self::Active,
            "user_paused" | "paused" | "Paused" => Self::UserPaused,
            "doom_loop_paused" => Self::UserPaused,
            "back_off_paused" => Self::BackOffPaused,
            "no_progress_paused" => Self::NoProgressPaused,
            "infra_paused" => Self::InfraPaused,
            "blocked" => Self::Blocked,
            "budget_limited" | "BudgetLimited" => Self::BudgetLimited,
            "complete" | "Complete" => Self::Complete,
            _ => Self::UserPaused,
        }
    }

    /// `true` for any paused variant.
    fn is_paused(&self) -> bool {
        matches!(
            self,
            Self::UserPaused | Self::BackOffPaused | Self::NoProgressPaused | Self::InfraPaused | Self::Blocked
        )
    }
}

/// Reason for pausing a goal. Maps 1:1 to one of the paused variants on
/// [`GoalStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalPauseReason {
    /// Paused by explicit user request.
    User,
    /// Paused after repeated classifier failures hit the back-off cap.
    BackOff,
    /// Paused because the verifier reported no progress on known gaps.
    NoProgress,
    /// Paused because the verifier determined the goal is not achievable.
    Verification,
    /// Paused due to an infrastructure error.
    Infra,
}

impl GoalPauseReason {
    fn to_status(self) -> GoalStatus {
        match self {
            Self::User => GoalStatus::UserPaused,
            Self::BackOff => GoalStatus::BackOffPaused,
            Self::NoProgress => GoalStatus::NoProgressPaused,
            Self::Verification => GoalStatus::Blocked,
            Self::Infra => GoalStatus::InfraPaused,
        }
    }

    fn history_detail(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::BackOff => "back_off",
            Self::NoProgress => "no_progress",
            Self::Verification => "blocked",
            Self::Infra => "infra",
        }
    }
}

/// Aggregate verdict produced by the goal-verification stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalClassifierVerdict {
    /// Goal was achieved.
    Achieved,
    /// Goal was not achieved.
    NotAchieved,
}

/// Lifecycle event recorded in the goal history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalEvent {
    /// Goal was created.
    GoalCreated,
    /// Planning phase started.
    PlanningStarted,
    /// Planning phase completed.
    PlanningCompleted,
    /// Planning phase failed.
    PlanningFailed,
    /// Worker started processing.
    WorkerStarted,
    /// Worker completed successfully.
    WorkerCompleted,
    /// Worker failed.
    WorkerFailed,
    /// Context was rotated.
    ContextRotated,
    /// Goal was paused.
    GoalPaused,
    /// Goal was resumed.
    GoalResumed,
    /// Goal completed successfully.
    GoalCompleted,
    /// Goal was cleared.
    GoalCleared,
    /// Budget was exceeded.
    BudgetExceeded,
    /// Premature stop was detected.
    PrematureStopDetected,
    /// Unknown or unrecognized event.
    #[serde(other)]
    Unknown,
}

/// A single history entry for a goal lifecycle event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalHistoryEntry {
    /// ISO-8601 timestamp of the event.
    timestamp: String,
    /// Lifecycle event type.
    event: GoalEvent,
    /// Optional human-readable detail string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// Optional round number associated with the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    round: Option<u32>,
    /// Optional token count at the time of the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tokens_used: Option<i64>,
    /// Unmet requirements or blockers recorded at this event.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unmet: Vec<String>,
}

impl GoalHistoryEntry {
    fn now(event: GoalEvent, detail: Option<String>) -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339(),
            event,
            detail,
            round: None,
            tokens_used: None,
            unmet: Vec::new(),
        }
    }
}

/// Full persisted state for a goal orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalOrchestration {
    /// Unique identifier for the goal.
    goal_id: String,
    /// Human-readable objective description.
    objective: String,
    /// Current lifecycle status.
    status: GoalStatus,
    /// Current execution phase.
    phase: GoalPhase,
    /// Optional token budget cap.
    token_budget: Option<i64>,
    /// Elapsed wall-clock time in milliseconds.
    elapsed_ms: u64,
    /// ISO-8601 creation timestamp.
    created_at: String,
    /// Currently executing subagent ID, if any.
    current_subagent_id: Option<String>,
    /// Role of the current subagent.
    current_subagent_role: Option<String>,
    /// Total worker rounds executed.
    #[serde(default)]
    total_worker_rounds: u32,
    /// Total verification rounds executed.
    #[serde(default)]
    total_verify_rounds: u32,
    /// Whether the budget limit notification has already been emitted.
    #[serde(skip)]
    budget_limit_reported: bool,
    /// Baseline token count when the goal started.
    #[serde(default)]
    token_baseline: i64,
    /// High-water mark for tokens used.
    #[serde(default)]
    tokens_used_high_water: i64,
    /// Tokens spent by the parent session before this goal started.
    #[serde(default)]
    parent_tokens_spent: i64,
    /// Last observed session token count.
    #[serde(default)]
    last_session_tokens_seen: Option<i64>,
    /// Ordered history of lifecycle events.
    history: Vec<GoalHistoryEntry>,
    /// User-facing pause message, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pause_message: Option<String>,
    /// Number of consecutive classifier stall detections.
    #[serde(default)]
    classifier_stall_count: u32,
    /// Total classifier run attempts.
    #[serde(default)]
    classifier_runs_attempted: u32,
    /// Rounds since the last verification pass.
    #[serde(default)]
    rounds_since_verify: u32,
    /// Consecutive not-achieved verdicts.
    #[serde(default)]
    consecutive_not_achieved: u32,
    /// Turn at which the strategist last fired.
    #[serde(default)]
    last_strategist_fired_at: u32,
    /// Bonus tokens granted by the strategist.
    #[serde(default)]
    strategist_cap_bonus: u32,
    /// Path of the last strategy recommendation, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_strategy_path: Option<String>,
    /// Last strategy recommendation text, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_strategy_recommendation: Option<String>,
    /// Commit hash at the last strategy change baseline, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    changes_baseline_commit: Option<String>,
    /// Fingerprint of the last gap signature, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_gap_fingerprint: Option<String>,
    /// Live token count for active subagents.
    #[serde(skip)]
    live_subagent_tokens: u64,
    /// Live token usage broken down by model.
    #[serde(skip)]
    live_tokens_by_model: Vec<(String, u64)>,
    /// Live context window size in tokens.
    #[serde(skip)]
    live_context_window: u64,
    /// Live context window utilization percentage (0-100).
    #[serde(skip)]
    live_context_pct: u8,
    /// Live turn count for the current goal.
    #[serde(skip)]
    live_turn_count: u32,
    /// Live tool call count for the current goal.
    #[serde(skip)]
    live_tool_call_count: u32,
    /// Whether a planning pass is currently in flight.
    #[serde(skip)]
    planning_in_flight: bool,
    /// Whether a verification pass is currently in flight.
    #[serde(skip)]
    verifying_in_flight: bool,
}

impl GoalOrchestration {
    fn reset_strategist_fields(&mut self) {
        self.consecutive_not_achieved = 0;
        self.last_strategist_fired_at = 0;
        self.strategist_cap_bonus = 0;
        self.last_strategy_path = None;
        self.last_strategy_recommendation = None;
    }

    fn reset_classifier_stall_fields(&mut self) {
        self.classifier_stall_count = 0;
        self.last_gap_fingerprint = None;
    }
}

/// Pure state machine for goal tracking.
#[derive(Debug)]
pub struct GoalTracker {
    /// Current goal orchestration state, if a goal is active.
    orchestration: Option<GoalOrchestration>,
    /// Directory where progress snapshots are persisted.
    session_dir: PathBuf,
    /// Instant when the goal became active.
    active_since: Option<Instant>,
}

impl GoalTracker {
    /// Create a new tracker with no active goal.
    fn new(session_dir: PathBuf) -> Self {
        Self {
            orchestration: None,
            session_dir,
            active_since: None,
        }
    }

    /// Restore tracker state from a persisted snapshot.
    pub fn from_snapshot(session_dir: PathBuf, mut snapshot: GoalOrchestration) -> Self {
        match snapshot.phase {
            GoalPhase::Planning | GoalPhase::Executing => {
                snapshot.phase = GoalPhase::Idle;
                if snapshot.status == GoalStatus::Active {
                    snapshot.status = GoalStatus::UserPaused;
                }
                snapshot.current_subagent_id = None;
                snapshot.current_subagent_role = None;
            }
            GoalPhase::Idle => {}
        }
        snapshot.planning_in_flight = false;
        snapshot.verifying_in_flight = false;
        let active_since = if snapshot.status == GoalStatus::Active {
            Some(Instant::now())
        } else {
            None
        };
        Self {
            orchestration: Some(snapshot),
            session_dir,
            active_since,
        }
    }

    /// Return an immutable reference to the current orchestration snapshot.
    fn snapshot(&self) -> Option<&GoalOrchestration> {
        self.orchestration.as_ref()
    }

    /// Return a mutable reference to the current orchestration snapshot.
    pub fn snapshot_mut(&mut self) -> Option<&mut GoalOrchestration> {
        self.orchestration.as_mut()
    }

    /// `true` if a goal is currently active.
    fn is_active(&self) -> bool {
        self.orchestration.as_ref().is_some_and(|o| o.status == GoalStatus::Active)
    }

    /// Current execution phase, if a goal is active.
    fn phase(&self) -> Option<GoalPhase> {
        self.orchestration.as_ref().map(|o| o.phase)
    }

    /// Current lifecycle status, if a goal is active.
    fn status(&self) -> Option<GoalStatus> {
        self.orchestration.as_ref().map(|o| o.status)
    }

    /// ID of the currently executing subagent, if any.
    fn current_subagent_id(&self) -> Option<&str> {
        self.orchestration.as_ref().and_then(|o| o.current_subagent_id.as_deref())
    }

    /// Human-readable objective, if a goal is active.
    fn objective(&self) -> Option<&str> {
        self.orchestration.as_ref().map(|o| o.objective.as_str())
    }

    /// Token budget cap, if set.
    fn token_budget(&self) -> Option<i64> {
        self.orchestration.as_ref().and_then(|o| o.token_budget)
    }

    /// Create a new goal. Replaces any existing orchestration.
    fn create_goal(
        &mut self,
        goal_id: String,
        objective: String,
        token_budget: Option<i64>,
        token_baseline: i64,
        created_at: String,
        baseline_commit: Option<String>,
    ) {
        let _ = std::fs::create_dir_all(self.goal_dir());
        if self.orchestration.is_some() {
            self.remove_scratch_root();
        }
        self.orchestration = Some(GoalOrchestration {
            goal_id,
            objective,
            status: GoalStatus::Active,
            phase: GoalPhase::Executing,
            token_budget,
            elapsed_ms: 0,
            created_at,
            current_subagent_id: None,
            current_subagent_role: None,
            total_worker_rounds: 0,
            total_verify_rounds: 0,
            budget_limit_reported: false,
            token_baseline,
            tokens_used_high_water: 0,
            parent_tokens_spent: 0,
            last_session_tokens_seen: Some(token_baseline),
            history: Vec::new(),
            pause_message: None,
            classifier_stall_count: 0,
            classifier_runs_attempted: 0,
            rounds_since_verify: 0,
            consecutive_not_achieved: 0,
            last_strategist_fired_at: 0,
            strategist_cap_bonus: 0,
            last_strategy_path: None,
            last_strategy_recommendation: None,
            changes_baseline_commit: baseline_commit,
            last_gap_fingerprint: None,
            live_subagent_tokens: 0,
            live_tokens_by_model: Vec::new(),
            live_context_window: 0,
            live_context_pct: 0,
            live_turn_count: 0,
            live_tool_call_count: 0,
            planning_in_flight: false,
            verifying_in_flight: false,
        });
        self.active_since = Some(Instant::now());
        self.record_event(GoalEvent::GoalCreated, None);
    }

    /// Update the execution phase of the active goal.
    fn set_phase(&mut self, phase: GoalPhase) {
        if let Some(o) = &mut self.orchestration {
            o.phase = phase;
        }
    }

    /// Update the current subagent ID and role.
    fn set_current_subagent(&mut self, id: Option<String>, role: Option<String>) {
        if let Some(o) = &mut self.orchestration {
            o.current_subagent_id = id;
            o.current_subagent_role = role;
        }
    }

    /// Pause the goal with a specific reason. Only transitions from `Active`.
    fn pause(&mut self, reason: GoalPauseReason) -> bool {
        self.pause_inner(reason, None)
    }

    /// Like [`Self::pause`] but also stores a human-readable `message`.
    fn pause_with_message(&mut self, reason: GoalPauseReason, message: String) -> bool {
        self.pause_inner(reason, Some(message))
    }

    fn pause_inner(&mut self, reason: GoalPauseReason, message: Option<String>) -> bool {
        let applied = if let Some(o) = &mut self.orchestration
            && o.status == GoalStatus::Active
        {
            if let Some(since) = self.active_since.take() {
                o.elapsed_ms = o.elapsed_ms.saturating_add(since.elapsed().as_millis() as u64);
            }
            o.status = reason.to_status();
            if message.is_some() {
                o.pause_message = message;
            }
            true
        } else {
            false
        };
        if applied {
            self.record_event(GoalEvent::GoalPaused, Some(reason.history_detail().to_owned()));
        }
        applied
    }

    /// Resume a paused goal (any paused variant). Returns `true` if applied.
    fn resume(&mut self) -> bool {
        if let Some(o) = &mut self.orchestration
            && o.status.is_paused()
        {
            o.status = GoalStatus::Active;
            o.pause_message = None;
            o.classifier_runs_attempted = 0;
            o.rounds_since_verify = 0;
            o.reset_strategist_fields();
            o.reset_classifier_stall_fields();
            self.active_since = Some(Instant::now());
            self.record_event(GoalEvent::GoalResumed, None);
            return true;
        }
        false
    }

    /// Mark the goal as complete. Accepts `Active` or any paused variant.
    fn complete(&mut self) -> bool {
        if let Some(o) = &mut self.orchestration
            && (o.status == GoalStatus::Active || o.status.is_paused())
        {
            if let Some(since) = self.active_since.take() {
                o.elapsed_ms = o.elapsed_ms.saturating_add(since.elapsed().as_millis() as u64);
            }
            o.status = GoalStatus::Complete;
            o.phase = GoalPhase::Idle;
            o.current_subagent_id = None;
            o.current_subagent_role = None;
            o.pause_message = None;
            o.reset_strategist_fields();
            self.record_event(GoalEvent::GoalCompleted, None);
            return true;
        }
        false
    }

    /// Mark the goal as budget-limited. Accepts `Active` or any paused variant.
    fn budget_limit(&mut self) -> bool {
        if let Some(o) = &mut self.orchestration
            && (o.status == GoalStatus::Active || o.status.is_paused())
        {
            if let Some(since) = self.active_since.take() {
                o.elapsed_ms = o.elapsed_ms.saturating_add(since.elapsed().as_millis() as u64);
            }
            o.status = GoalStatus::BudgetLimited;
            o.phase = GoalPhase::Idle;
            o.current_subagent_id = None;
            o.current_subagent_role = None;
            o.pause_message = None;
            o.reset_strategist_fields();
            self.record_event(GoalEvent::BudgetExceeded, None);
            return true;
        }
        false
    }

    /// Clear the goal entirely.
    pub fn clear(&mut self) {
        self.orchestration = None;
        self.active_since = None;
    }

    fn goal_dir(&self) -> PathBuf {
        self.session_dir.join("goal")
    }

    fn remove_scratch_root(&self) {
        let _ = std::fs::remove_dir_all(self.session_dir.join("goal"));
    }

    /// Flush elapsed wall-clock time into `elapsed_ms`.
    pub fn account_elapsed(&mut self) {
        if let Some(o) = &mut self.orchestration
            && let Some(since) = self.active_since
        {
            o.elapsed_ms = o.elapsed_ms.saturating_add(since.elapsed().as_millis() as u64);
            self.active_since = Some(since);
        }
    }

    /// Record a `NotAchieved` rejection's gap `fingerprint` and report
    /// whether the goal has stalled.
    fn record_classifier_stall(&mut self, fingerprint: &str) -> bool {
        let Some(o) = self.orchestration.as_mut() else {
            return false;
        };
        if o.last_gap_fingerprint.as_deref() == Some(fingerprint) {
            o.classifier_stall_count = o.classifier_stall_count.saturating_add(1);
        } else {
            o.last_gap_fingerprint = Some(fingerprint.to_string());
            o.classifier_stall_count = 1;
        }
        o.classifier_stall_count >= 2
    }

    /// Undo the most recent attempt-slot reservation.
    pub fn rollback_classifier_attempt(&mut self) {
        if let Some(o) = self.orchestration.as_mut() {
            o.classifier_runs_attempted = o.classifier_runs_attempted.saturating_sub(1);
        }
    }

    /// Clear the stall streak.
    fn reset_classifier_stall(&mut self) {
        if let Some(o) = self.orchestration.as_mut() {
            o.reset_classifier_stall_fields();
        }
    }

    /// Increment the consecutive-`NotAchieved` streak and return the new value.
    fn record_not_achieved_streak(&mut self) -> u32 {
        match self.orchestration.as_mut() {
            Some(o) => {
                o.consecutive_not_achieved = o.consecutive_not_achieved.saturating_add(1);
                o.consecutive_not_achieved
            }
            None => 0,
        }
    }

    /// Atomically evaluate the strategist trigger and claim a fire.
    pub fn claim_strategist_fire(&mut self, should_fire: impl Fn(u32, u32) -> bool) -> Option<u32> {
        let o = self.orchestration.as_mut()?;
        if should_fire(o.consecutive_not_achieved, o.last_strategist_fired_at) {
            o.last_strategist_fired_at = o.consecutive_not_achieved;
            o.strategist_cap_bonus = 3;
            o.reset_classifier_stall_fields();
            Some(o.consecutive_not_achieved)
        } else {
            None
        }
    }

    /// Revoke the cap bonus granted by [`Self::claim_strategist_fire`].
    pub fn revoke_strategist_cap_bonus(&mut self) {
        if let Some(o) = self.orchestration.as_mut() {
            o.strategist_cap_bonus = 0;
        }
    }

    /// Reset ALL strategist state.
    fn reset_strategist_state(&mut self) {
        if let Some(o) = self.orchestration.as_mut() {
            o.reset_strategist_fields();
        }
    }

    /// Persist the strategist's latest output path + short recommendation.
    fn record_strategy_recommendation(&mut self, path: String, recommendation: String) {
        if let Some(o) = self.orchestration.as_mut() {
            o.last_strategy_path = Some(path);
            o.last_strategy_recommendation = Some(recommendation);
        }
    }

    /// Append a history entry to the active goal, if any.
    fn append_history(&mut self, entry: GoalHistoryEntry) {
        if let Some(o) = &mut self.orchestration {
            o.history.push(entry);
        }
    }

    fn record_event(&mut self, event: GoalEvent, detail: Option<String>) {
        self.append_history(GoalHistoryEntry::now(event, detail));
    }
}

#[cfg(test)]
mod goal_tracker_tests;
#[cfg(test)]
mod progress_tests;
