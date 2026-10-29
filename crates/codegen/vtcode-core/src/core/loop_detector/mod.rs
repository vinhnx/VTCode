//! Loop detection for agent operations
//!
//! Detects when the agent is stuck in repetitive patterns and suggests intervention.
//! The unified interactive VT Code runloop applies its own richer turn-local
//! recovery policy; this detector remains the generic safeguard for legacy or
//! non-unified autonomous execution paths.
//!
//! Structure:
//! - `mod.rs` — the `LoopDetector` state machine: budgets, streak tracking,
//!   identical-call enforcement, pattern detection, and the public API.
//! - `normalization` — pure helper functions: tool classification, argument
//!   hashing/normalization, and read-target extraction. Kept separate so the
//!   detector state and the normalization logic are independently testable.

use crate::config::constants::{defaults, tools};
use hashbrown::{HashMap, HashSet};
use std::collections::VecDeque;
use std::time::Instant;

mod normalization;
use self::normalization::*;

// Separate limits for different operation types to reduce false positives
const MAX_READONLY_TOOL_CALLS: usize = 10; // read_file, grep_file, list_files
const MAX_WRITE_TOOL_CALLS: usize = 3; // write_file, edit_file, apply_patch
const MAX_COMMAND_TOOL_CALLS: usize = 8; // shell, command_session (raised: verification commands are legitimate)
const MAX_OTHER_TOOL_CALLS: usize = 3; // Other tools (default)
const DETECTION_WINDOW: usize = 20; // Raised from 10 to catch cross-batch duplicates
const HARD_LIMIT_MULTIPLIER: usize = 2; // Hard stop at 2x soft limit
const MAX_SIMILAR_READ_TARGET_CALLS: usize = 4;
const MAX_SIMILAR_READ_TARGET_VARIANTS: usize = 3;

/// Sliding-window repeat threshold per Equation 18.15.
/// When the same (tool, normalized_args) hash appears more than this many
/// times within the detection window, a loop warning is emitted.
const SLIDING_WINDOW_MAX_REPEATS: usize = 3;

/// Global hard limit on total read-only tool calls across ALL read-only tools.
/// Prevents the agent from alternating between different read-only tools to
/// evade per-tool limits. Fires a hard stop when exhausted.
///
/// Also referenced by `prompts::harness_limits` to advertise the budget in the
/// system prompt; harness_limits imports this const, so there is no second copy.
pub(crate) const MAX_TOTAL_READONLY_CALLS: usize = 30;

/// Subagent-specific read-only budget. Subagents should do focused work and
/// need less exploration headroom than the main agent.
///
/// Also referenced by `subagents::config` to state the budget in writable
/// child instructions; config imports this const, so there is no second copy.
pub(crate) const SUBAGENT_MAX_TOTAL_READONLY_CALLS: usize = 20;

/// Navigation streak thresholds -- warning and hard stop.
/// Subagents get tighter limits to force earlier synthesis.
const NAVIGATION_WARNING_STREAK: usize = 4;
const NAVIGATION_HARD_STOP_STREAK: usize = 7;
const SUBAGENT_NAVIGATION_WARNING_STREAK: usize = 3;
pub(crate) const SUBAGENT_NAVIGATION_HARD_STOP_STREAK: usize = 5;

/// Prefix of every message returned by [`LoopDetector::record_call`] when the
/// call reached a hard limit (the caller then sees
/// [`LoopDetector::is_hard_limit_exceeded`] for that tool). Callers end the run
/// or force a tool-free synthesis pass, so each message states what is blocked
/// and what to do with the results already gathered.
pub const HARD_STOP_PREFIX: &str = "Loop limit reached:";

const LEGACY_GREP_FILE: &str = tools::GREP_FILE;
const LEGACY_LIST_FILES: &str = tools::LIST_FILES;
const LEGACY_SEARCH_TOOLS: &str = "search_tools";

#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub args_hash: u64,
    pub read_target: Option<String>,
    pub timestamp: Instant,
}

#[derive(Debug)]
pub struct LoopDetector {
    recent_calls: VecDeque<ToolCallRecord>,
    tool_counts: HashMap<String, usize>,
    hard_stopped_tools: HashSet<String>,
    last_warning: Option<Instant>,
    max_identical_call_limit: Option<usize>,
    custom_limits: HashMap<String, usize>,
    /// Cache mapping (tool_name, raw_args) composite hash → normalized_args_hash.
    /// Avoids re-running normalization on repeated identical calls.
    norm_cache: HashMap<u64, u64>,
    /// Insertion order for LRU eviction of norm_cache (ring buffer of keys).
    norm_cache_order: VecDeque<u64>,
    /// Tracks consecutive read-only calls without any write/execution progress.
    /// Resets on any mutating tool call.
    readonly_streak: usize,
    /// Cumulative count of all read-only tool calls since last reset.
    /// Unlike `readonly_streak`, this never resets on mutating calls -- it only
    /// clears on `reset()`.  Used to enforce a global read-only budget so the
    /// agent cannot alternate between different read-only tools to evade
    /// per-tool limits.
    total_readonly_calls: usize,
    /// When true, applies tighter read-only budgets and navigation streak
    /// thresholds appropriate for subagents that should do focused work.
    is_subagent: bool,
    /// Sliding window of composite (tool, args_hash) values for deduplication
    /// per Equation 18.15. Used to detect non-consecutive repeated actions.
    action_hash_window: VecDeque<u64>,
}

impl LoopDetector {
    /// Create a new detector with the default repeated-call limit.
    pub fn new() -> Self {
        Self::with_max_repeated_calls(defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS)
    }

    /// Create a new detector with a custom repeated-call limit.
    pub fn with_max_repeated_calls(limit: usize) -> Self {
        let normalized_limit = (limit > 1).then_some(limit);
        Self {
            recent_calls: VecDeque::with_capacity(DETECTION_WINDOW),
            tool_counts: HashMap::new(),
            hard_stopped_tools: HashSet::new(),
            last_warning: None,
            max_identical_call_limit: normalized_limit,
            custom_limits: HashMap::new(),
            norm_cache: HashMap::with_capacity(128),
            norm_cache_order: VecDeque::with_capacity(128),
            readonly_streak: 0,
            total_readonly_calls: 0,
            is_subagent: false,
            action_hash_window: VecDeque::with_capacity(DETECTION_WINDOW),
        }
    }

    /// Configure this detector for subagent use with tighter read-only budgets
    /// and earlier navigation streak intervention.
    pub fn set_subagent_mode(&mut self, is_subagent: bool) {
        self.is_subagent = is_subagent;
    }

    /// Returns the effective global read-only budget for this detector.
    fn effective_readonly_budget(&self) -> usize {
        if self.is_subagent {
            SUBAGENT_MAX_TOTAL_READONLY_CALLS
        } else {
            MAX_TOTAL_READONLY_CALLS
        }
    }

    /// Returns the effective navigation warning and hard-stop streak thresholds.
    fn effective_navigation_thresholds(&self) -> (usize, usize) {
        if self.is_subagent {
            (SUBAGENT_NAVIGATION_WARNING_STREAK, SUBAGENT_NAVIGATION_HARD_STOP_STREAK)
        } else {
            (NAVIGATION_WARNING_STREAK, NAVIGATION_HARD_STOP_STREAK)
        }
    }

    /// Set a custom limit for a specific tool.
    /// This overrides the default category-based limits.
    pub fn set_tool_limit(&mut self, tool_name: &str, limit: usize) {
        self.custom_limits.insert(tool_name.to_string(), limit);
    }

    /// Record a tool call and return a warning or hard-stop message if a loop is detected.
    pub fn record_call(&mut self, tool_name: &str, args: &serde_json::Value) -> Option<String> {
        let warning = self.record_call_inner(tool_name, args);
        if warning.as_deref().is_some_and(|message| message.starts_with(HARD_STOP_PREFIX)) {
            self.hard_stopped_tools.insert(tool_name.to_owned());
        }
        warning
    }

    fn record_call_inner(&mut self, tool_name: &str, args: &serde_json::Value) -> Option<String> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut raw_hasher = DefaultHasher::new();
        tool_name.hash(&mut raw_hasher);
        if let Ok(bytes) = serde_json::to_vec(args) {
            bytes.hash(&mut raw_hasher);
        } else {
            args.to_string().hash(&mut raw_hasher);
        }
        let raw_key = raw_hasher.finish();

        let args_hash = if let Some(&cached) = self.norm_cache.get(&raw_key) {
            cached
        } else {
            // Use streaming hash — no clone, no serialize of the full JSON.
            let hash = hash_normalized_args(tool_name, args);
            // LRU eviction: remove oldest entry when cache is full
            const NORM_CACHE_CAPACITY: usize = 128;
            if self.norm_cache.len() >= NORM_CACHE_CAPACITY {
                if let Some(oldest_key) = self.norm_cache_order.pop_front() {
                    self.norm_cache.remove(&oldest_key);
                }
            }
            self.norm_cache.insert(raw_key, hash);
            self.norm_cache_order.push_back(raw_key);
            hash
        };

        // For command tools, enforce identical-call limits when the normalized
        // command is a verification or read pattern (e.g., `command -v X`, `cat file`).
        // Regular commands (e.g., `cargo check`) are exempt because re-running
        // them is legitimate (build-test cycles).
        let enforce_identical = Self::should_enforce_identical_limit(tool_name)
            || (is_command_tool_name(base_tool_name(tool_name))
                && Self::is_verification_or_read_command(tool_name, args));

        if let Some(limit) = self.max_identical_call_limit
            && enforce_identical
        {
            let required_history = limit.saturating_sub(1);
            if required_history > 0 && self.recent_calls.len() >= required_history {
                let identical = self
                    .recent_calls
                    .iter()
                    .rev()
                    .take(required_history)
                    .all(|record| record.tool_name == tool_name && record.args_hash == args_hash);

                if identical {
                    // Escalate to hard limit so callers halt immediately.
                    let hard_limit = self.get_limit_for_tool(tool_name) * HARD_LIMIT_MULTIPLIER;
                    self.tool_counts.insert(tool_name.to_string(), hard_limit);

                    return Some(format!(
                        "{HARD_STOP_PREFIX} {tool_name} was called {limit} times in a row with identical arguments, \
                         so further calls to it are blocked. Use the result already in the conversation, or change the approach."
                    ));
                }
            }
        }

        let record = ToolCallRecord {
            tool_name: tool_name.to_string(),
            args_hash,
            read_target: read_target_for_tool_call(tool_name, args),
            timestamp: Instant::now(),
        };

        if self.recent_calls.len() >= DETECTION_WINDOW
            && let Some(old) = self.recent_calls.pop_front()
            && let Some(count) = self.tool_counts.get_mut(&old.tool_name)
        {
            *count = count.saturating_sub(1);
        }

        self.recent_calls.push_back(record);
        // Use get_mut + insert to avoid String allocation on every call.
        // entry() would allocate tool_name.to_string() even for existing keys.
        match self.tool_counts.get_mut(tool_name) {
            Some(count) => *count += 1,
            None => {
                self.tool_counts.insert(tool_name.to_string(), 1);
            }
        }

        if let Some(read_target_warning) = self.detect_repetitive_read_target(tool_name) {
            return Some(read_target_warning);
        }

        // --- Navigation Loop Detection (NL2Repo-Bench integration) ---
        let base_name = base_tool_name(tool_name);
        let current_read_target = self.recent_calls.back().and_then(|r| r.read_target.clone());
        let is_readonly = matches!(base_name, tools::READ_FILE | LEGACY_GREP_FILE | LEGACY_LIST_FILES | tools::CODE_SEARCH)
                || (base_name == tools::UNIFIED_FILE && current_read_target.is_some())
                // Command tools with a read target (e.g. `cat file`) are readonly
                // exploration; command tools without one (`cargo check`) are
                // mutating/verification actions that break the streak.
                || (is_command_tool_name(base_name) && current_read_target.is_some());

        let is_mutating =
            matches!(base_name, tools::WRITE_FILE | tools::CREATE_FILE | tools::EDIT_FILE | tools::APPLY_PATCH)
                || (base_name == tools::UNIFIED_EXEC && current_read_target.is_none())
                || (is_command_tool_name(base_name) && current_read_target.is_none());

        if is_readonly {
            self.readonly_streak += 1;
            self.total_readonly_calls += 1;
        } else if is_mutating {
            self.readonly_streak = 0;
        }

        // --- Global read-only budget ---
        // Prevents the agent from alternating between different read-only tools
        // (e.g., code_search and file_operation) to evade per-tool limits.
        // The total only resets with the detector (each turn), so once it is
        // exhausted every later call this turn hits this stop, whatever its kind.
        let readonly_budget = self.effective_readonly_budget();
        if self.total_readonly_calls >= readonly_budget {
            let hard_limit = self.get_limit_for_tool(tool_name) * HARD_LIMIT_MULTIPLIER;
            self.tool_counts.insert(tool_name.to_string(), hard_limit);
            return Some(format!(
                "{HARD_STOP_PREFIX} the global read-only budget is exhausted ({} total read-only calls, limit: {}), \
                 so every further tool call this turn is blocked, including edits and commands. Write the final \
                 answer from the results already in the conversation.",
                self.total_readonly_calls, readonly_budget
            ));
        }

        let (warning_streak, hard_stop_streak) = self.effective_navigation_thresholds();
        if self.readonly_streak >= warning_streak {
            if self.readonly_streak >= hard_stop_streak {
                let hard_limit = self.get_limit_for_tool(tool_name) * HARD_LIMIT_MULTIPLIER;
                self.tool_counts.insert(tool_name.to_string(), hard_limit);
                return Some(format!(
                    "{HARD_STOP_PREFIX} {} consecutive read-only calls without an edit or command, \
                     so further read-only calls are blocked. Write the final answer from the results already in \
                     the conversation.",
                    self.readonly_streak
                ));
            }

            let msg = format!(
                "Navigation loop detected: {} consecutive read-only calls without an edit or command. \
                 At {hard_stop_streak} in a row, further read-only calls are blocked. If the results already in the \
                 conversation are enough, produce the answer or implementation now; re-reading files or \
                 re-running searches already performed returns nothing new. If a tool output was truncated, \
                 use the exact continuation range, or use `exec_command.cmd` with `cat` for full content.",
                self.readonly_streak
            );
            let now = Instant::now();
            let should_warn = self
                .last_warning
                .map(|last| now.duration_since(last).as_secs() > 30)
                .unwrap_or(true);

            if should_warn {
                self.last_warning = Some(now);
                return Some(msg);
            }
        }

        let count_warning = self.check_for_loops(tool_name);
        if count_warning
            .as_deref()
            .is_some_and(|message| message.starts_with(HARD_STOP_PREFIX))
        {
            return count_warning;
        }
        if let Some(pattern_warning) = self.detect_patterns() {
            return Some(pattern_warning);
        }

        // --- Sliding-window action hash deduplication (Equation 18.15) ---
        // Detect when the same (tool, normalized_args) hash appears multiple
        // times within the detection window, even if not consecutively (e.g.,
        // A→B→A→C→A). Runs after all other detectors as a last-resort check
        // so more specific detectors (read target, navigation, patterns) fire
        // first.
        let enforce_identical = Self::should_enforce_identical_limit(tool_name)
            || (is_command_tool_name(base_tool_name(tool_name))
                && Self::is_verification_or_read_command(tool_name, args));
        if enforce_identical {
            use std::hash::{Hash, Hasher};
            let mut composite_hasher = DefaultHasher::new();
            tool_name.hash(&mut composite_hasher);
            args_hash.hash(&mut composite_hasher);
            let composite_hash = composite_hasher.finish();

            // Maintain the sliding window
            if self.action_hash_window.len() >= DETECTION_WINDOW {
                self.action_hash_window.pop_front();
            }

            // Count non-consecutive occurrences: skip trailing identical entries
            // so A→A→A counts as 0, while A→B→A→B→A counts as 2.
            let count_in_window = self
                .action_hash_window
                .iter()
                .rev()
                .skip_while(|&&h| h == composite_hash)
                .filter(|&&h| h == composite_hash)
                .count() as u32;

            self.action_hash_window.push_back(composite_hash);

            if count_in_window >= SLIDING_WINDOW_MAX_REPEATS as u32 {
                return Some(format!(
                    "Warning: Same action ({tool_name}) detected {} times (non-consecutive) within \
                     the last {} operations. You appear to be repeating the same operation without \
                     making progress. Consider re-planning your approach.",
                    count_in_window + 1,
                    DETECTION_WINDOW,
                ));
            }
        }

        count_warning
    }

    fn check_for_loops(&mut self, tool_name: &str) -> Option<String> {
        // Shell and tracker tools multiplex distinct actions. Count normalized
        // repetitions so updates to different tracker items/states are progress.
        // Explicit operator limits retain their aggregate meaning.
        let count = if (is_command_tool_name(base_tool_name(tool_name))
            || base_tool_name(tool_name) == tools::TASK_TRACKER)
            && !self.custom_limits.contains_key(tool_name)
            && !self.custom_limits.contains_key(base_tool_name(tool_name))
        {
            self.recent_calls.back().map_or(0, |latest| {
                self.recent_calls
                    .iter()
                    .filter(|call| call.tool_name == tool_name && call.args_hash == latest.args_hash)
                    .count()
            })
        } else {
            self.tool_counts.get(tool_name).copied().unwrap_or(0)
        };

        // Determine tool-specific limits
        let max_calls = self.get_limit_for_tool(tool_name);

        // Hard limit check - immediate halt
        let hard_limit = max_calls * HARD_LIMIT_MULTIPLIER;
        if count >= hard_limit {
            return Some(format!(
                "{HARD_STOP_PREFIX} '{tool_name}' was called {count} times (hard limit: {hard_limit}), \
                 so further calls to it are blocked. Change the approach, or ask the user for guidance."
            ));
        }

        // Soft limit - warning with cooldown and alternative suggestions
        if count >= max_calls {
            let now = Instant::now();
            let should_warn = self
                .last_warning
                .map(|last| now.duration_since(last).as_secs() > 30)
                .unwrap_or(true);

            if should_warn {
                self.last_warning = Some(now);
                let alternatives = Self::suggest_alternative_for_tool(tool_name);

                return Some(format!(
                    "Loop detected: '{tool_name}' called {count} times in last {DETECTION_WINDOW} operations.\n\n\
                     {alternatives}\n\n\
                     Hard limit at {hard_limit} calls."
                ));
            }
        }

        None
    }

    fn detect_repetitive_read_target(&mut self, tool_name: &str) -> Option<String> {
        let base_name = base_tool_name(tool_name);
        // The current call's record was already pushed into `recent_calls` with
        // `read_target` populated by `read_target_for_tool_call` (which now
        // handles `file_operation` with `action: "read"` and command tools with
        // `__read__:path` normalization). Use that field as the authoritative
        // indicator instead of checking for a `::read` suffix.
        let current_has_read_target = self.recent_calls.back().is_some_and(|r| r.read_target.is_some());
        let is_read_tool = base_name == tools::READ_FILE
            || (base_name == tools::UNIFIED_FILE && current_has_read_target)
            || (is_command_tool_name(base_name) && current_has_read_target);
        if !is_read_tool {
            return None;
        }

        // Find the current read target from the most recent read_tool call,
        // not just the last call (which might be a grep with no read_target).
        let current_target = self
            .recent_calls
            .iter()
            .rev()
            .find(|record| record.read_target.is_some())
            .and_then(|record| record.read_target.as_deref())?;

        // Count read_file calls on the same target in recent history, skipping over
        // other read-only tools (grep, list, search) that don't reset the streak.
        // Command tools with a read_target (e.g. `cat file`) are counted as reads.
        // Only mutating tools (write, edit, patch) and non-read commands break the streak.
        let mut same_target_streak = 0usize;
        let mut variants = HashSet::new();
        for record in self.recent_calls.iter().rev() {
            let rec_base = base_tool_name(&record.tool_name);
            let rec_has_read_target = record.read_target.is_some();
            let rec_is_read_tool = rec_base == tools::READ_FILE
                || (rec_base == tools::UNIFIED_FILE && rec_has_read_target)
                || (is_command_tool_name(rec_base) && rec_has_read_target);
            // Command tools without a read_target are mutating (e.g., `cargo check`)
            let rec_is_mutating =
                matches!(rec_base, tools::WRITE_FILE | tools::CREATE_FILE | tools::EDIT_FILE | tools::APPLY_PATCH)
                    || (is_command_tool_name(rec_base) && !rec_has_read_target);

            if rec_is_mutating {
                break;
            }

            if rec_is_read_tool && record.read_target.as_deref() == Some(current_target) {
                same_target_streak += 1;
                variants.insert(record.args_hash);
            }
        }

        if same_target_streak >= MAX_SIMILAR_READ_TARGET_CALLS && variants.len() <= MAX_SIMILAR_READ_TARGET_VARIANTS {
            let hard_limit = self.get_limit_for_tool(tool_name) * HARD_LIMIT_MULTIPLIER;
            self.tool_counts.insert(tool_name.to_string(), hard_limit);
            return Some(format!(
                "{HARD_STOP_PREFIX} repeated '{}' calls for '{}' with minimal argument variation \
                 ({}-call streak, {} variants), so further calls to it are blocked. The earlier reads of this \
                 file are already in the conversation and likely hold what you need. If a read was truncated, \
                 use `exec_command.cmd` with `cat {}` for full content, or use the exact continuation range.",
                tool_name,
                current_target,
                same_target_streak,
                variants.len(),
                current_target
            ));
        }

        None
    }

    /// Check if hard limit is exceeded (should halt execution)
    pub fn is_hard_limit_exceeded(&self, tool_name: &str) -> bool {
        self.hard_stopped_tools.contains(tool_name)
    }

    /// Get current call count for a tool
    pub fn get_call_count(&self, tool_name: &str) -> usize {
        self.tool_counts.get(tool_name).copied().unwrap_or(0)
    }

    /// Get cumulative read-only call count since last reset.
    pub fn total_readonly_calls(&self) -> usize {
        self.total_readonly_calls
    }

    /// Reset tracking for specific tool (use after successful progress)
    pub fn reset_tool(&mut self, tool_name: &str) {
        self.tool_counts.remove(tool_name);
        self.hard_stopped_tools.remove(tool_name);
        self.recent_calls.retain(|r| r.tool_name != tool_name);
    }

    /// Suggest alternative approaches for common loop patterns
    /// Only called after loop detection, so `#[cold]`.
    #[cold]
    pub fn suggest_alternative(&self, tool_name: &str) -> Option<String> {
        match tool_name {
            LEGACY_LIST_FILES => Some(
                "Instead of listing files repeatedly:\n\
                 • Use code_search with a specific query and narrow file_types for code patterns\n\
                 • Use exec_command.cmd with rg for raw text, docs, or logs\n\
                 • Target specific subdirectories (e.g., 'src/', 'tests/')\n\
                 • Use exec_command.cmd with sed or cat if you know the exact file path"
                    .to_string(),
            ),
            LEGACY_GREP_FILE => Some(
                "Instead of grepping repeatedly:\n\
                 • If syntax matters, switch to code_search and request definition or usage results\n\
                 • Refine your text pattern or narrow the path when grep is the right tool\n\
                 • Use exec_command.cmd with sed or cat to examine specific files\n\
                 • Consider a small script through exec_command.cmd for complex filtering"
                    .to_string(),
            ),
            tools::READ_FILE => Some(
                "Instead of reading files repeatedly:\n\
                 • Use code_search with a specific query, path, and result_types for code lookups\n\
                 • Use exec_command.cmd with rg to find specific content first\n\
                 • Read specific line ranges with sed through exec_command.cmd\n\
                 • Consider if you already have the information needed"
                    .to_string(),
            ),
            LEGACY_SEARCH_TOOLS => Some(
                "Instead of searching tools repeatedly:\n\
                 • Review the tools you've already discovered\n\
                 • Use the dedicated MCP or deferred discovery affordance when available\n\
                 • Check if you need a different approach to the task"
                    .to_string(),
            ),
            _ => Some(
                "Shift focus to root cause analysis rather than patching symptoms. Re-evaluate planning assumptions specifically regarding environmental constraints. Consider:\n\
                 • Verifying environment state (`env`, `ls -la`, `which <cmd>`) before more code edits\n\
                 • Breaking down the problem into smaller, verifiable sub-tasks\n\
                 • Checking if a recent change introduced a regression (run existing tests)\n\
                 • Asking for user guidance if strategic direction is ambiguous"
                    .to_string(),
            ),
        }
    }

    /// Get the number of tools currently being tracked
    pub fn get_tracked_tool_count(&self) -> usize {
        self.tool_counts.len()
    }

    /// Reset all tracking state.
    pub fn reset(&mut self) {
        self.recent_calls.clear();
        self.tool_counts.clear();
        self.hard_stopped_tools.clear();
        self.last_warning = None;
        self.norm_cache.clear();
        self.norm_cache_order.clear();
        self.action_hash_window.clear();
        self.readonly_streak = 0;
        self.total_readonly_calls = 0;
    }

    /// Reset only the read-only streak counter without clearing tool call history.
    /// Used during stall recovery to allow the agent to try a different strategy
    /// while still detecting re-entry into the same looping pattern.
    pub fn reset_readonly_streak(&mut self) {
        self.readonly_streak = 0;
        self.last_warning = None;
    }

    /// Get limit for a specific tool.
    /// Checks custom limits first, then falls back to category defaults.
    #[inline]
    fn get_limit_for_tool(&self, tool_name: &str) -> usize {
        if let Some(&limit) = self.custom_limits.get(tool_name) {
            return limit;
        }
        let base_name = base_tool_name(tool_name);
        if let Some(&limit) = self.custom_limits.get(base_name) {
            return limit;
        }

        if base_name == tools::UNIFIED_FILE {
            if let Some((_, action)) = tool_name.split_once("::") {
                return if action.eq_ignore_ascii_case("read") {
                    MAX_READONLY_TOOL_CALLS
                } else {
                    MAX_WRITE_TOOL_CALLS
                };
            }
            return MAX_READONLY_TOOL_CALLS;
        }

        match base_name {
            tools::READ_FILE | LEGACY_GREP_FILE | LEGACY_LIST_FILES | tools::CODE_SEARCH => MAX_READONLY_TOOL_CALLS,
            tools::WRITE_FILE | tools::EDIT_FILE | tools::APPLY_PATCH => MAX_WRITE_TOOL_CALLS,
            _ if is_command_tool_name(base_name) => MAX_COMMAND_TOOL_CALLS,
            _ => MAX_OTHER_TOOL_CALLS,
        }
    }

    #[inline]
    fn should_enforce_identical_limit(tool_name: &str) -> bool {
        let base_name = base_tool_name(tool_name);
        if is_write_tool_name(base_name) {
            // Write/edit tools are excluded: they legitimately repeat on the
            // same path with different content (write then fix), and
            // `hash_normalized_args` nulls their content, so different edits
            // hash identically and would trip a false HARD STOP.
            return false;
        }
        !is_command_tool_name(base_name)
    }

    /// Returns `true` when the command is a verification or file-read pattern
    /// that should be subject to identical-call limits.
    ///
    /// Verification commands (`command -v X`, `which X`, `X --help`) and
    /// simple file reads (`cat file`, `head file`) are low-value repeats.
    /// Regular commands (e.g., `cargo check`) are exempt.
    #[inline]
    fn is_verification_or_read_command(tool_name: &str, args: &serde_json::Value) -> bool {
        let base_name = base_tool_name(tool_name);
        if !is_command_tool_name(base_name) {
            return false;
        }
        // exec_command uses the `cmd` key; pty/dispatcher tools use `command`.
        if let Some(cmd) = args.get("cmd").or_else(|| args.get("command")).and_then(|v| v.as_str()) {
            return canonicalize_command_for_detection(cmd).is_some();
        }
        false
    }

    /// Suggest alternatives for stuck tools (extracted to static method for efficiency)
    /// Called only on the cold path (loop already detected); marked `#[cold]`
    /// and `#[inline(never)]` so LLVM does not inline it into the hot caller.
    #[cold]
    #[inline(never)]
    fn suggest_alternative_for_tool(tool_name: &str) -> String {
        match base_tool_name(tool_name) {
            LEGACY_LIST_FILES => "Instead of listing repeatedly:\n\
                 • Use code_search with a specific query and narrow file_types for code patterns\n\
                 • Use exec_command.cmd with rg for raw text, docs, or logs\n\
                 • Target specific subdirectories (e.g., 'src/', 'tests/')\n\
                 • Use exec_command.cmd with sed or cat if you know the exact file path"
                .to_string(),
            LEGACY_GREP_FILE => "Instead of grepping repeatedly:\n\
                 • If syntax matters, switch to code_search and request definition or usage results\n\
                 • Refine your text pattern or narrow the path when grep is the right tool\n\
                 • Use exec_command.cmd with sed or cat to examine specific files\n\
                 • Consider a small script through exec_command.cmd for complex filtering"
                .to_string(),
            tools::READ_FILE => "Instead of reading files repeatedly:\n\
                 • Use code_search with a specific query, path, and result_types for code lookups\n\
                 • Use exec_command.cmd with rg to find specific content first\n\
                 • Read specific line ranges with sed through exec_command.cmd\n\
                 • Consider if you already have the information needed"
                .to_string(),
            LEGACY_SEARCH_TOOLS => "Instead of searching tools repeatedly:\n\
                 • Review the tools you've already discovered\n\
                 • Use the dedicated MCP or deferred discovery affordance when available\n\
                 • Check if you need a different approach to the task"
                .to_string(),
            _ => "Shift focus to root cause analysis rather than patching symptoms. Re-evaluate planning assumptions specifically regarding environmental constraints. Consider:\n\
                 • Verifying environment state (`env`, `ls -la`, `which <cmd>`) before more code edits\n\
                 • Breaking down the problem into smaller, verifiable sub-tasks\n\
                 • Checking if a recent change introduced a regression (run existing tests)\n\
                 • Asking for user guidance if strategic direction is ambiguous"
                .to_string(),
        }
    }

    /// Detect complex repetitive patterns (e.g. A -> B -> A -> B)
    fn detect_patterns(&self) -> Option<String> {
        let history: Vec<(&str, u64)> = self.recent_calls.iter().map(|r| (r.tool_name.as_str(), r.args_hash)).collect();

        let len = history.len();
        if len < 4 {
            return None;
        }

        // Check for patterns of length K where 2*K <= len
        // We look for imminent repetition: [.. A, B, A, B]
        for k in 2..=(len / 2) {
            let suffix = &history[len - k..];
            let prev = &history[len - 2 * k..len - k];

            if suffix == prev {
                let pattern_desc: Vec<&str> = suffix.iter().map(|(name, _)| *name).collect();
                let pattern_str = pattern_desc.join(" -> ");

                return Some(format!(
                    "Repetitive pattern detected: [{pattern_str}]\n\
                     The agent appears to be cycling through the same actions. \
                     Please pause and reassess the strategy."
                ));
            }

            // Fuzzy detection: if tool names match but hashes differ, check semantic similarity?
            // For now, simpler fuzzy check: ignore edit_file content arguments?
            // Better: Detecting "oscillating" behavior A->B->A->B even if args slightly differ.
            // If tool names match exactly for a sequence of length >= 3
            let suffix_names: Vec<&str> = suffix.iter().map(|(n, _)| *n).collect();
            let prev_names: Vec<&str> = prev.iter().map(|(n, _)| *n).collect();

            let has_distinct_tools = suffix_names
                .first()
                .is_some_and(|first| suffix_names.iter().any(|name| name != first));
            if suffix_names == prev_names
                && has_distinct_tools
                && !suffix_names.iter().any(|name| is_command_tool_name(base_tool_name(name)))
            {
                return Some(format!(
                    "Oscillating tool pattern detected: [{}]\n\
                     The agent is repeating the same sequence of tools. \
                     Ensure you are making actual progress.",
                    suffix_names.join(" -> ")
                ));
            }
        }

        None
    }
}

impl Default for LoopDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
