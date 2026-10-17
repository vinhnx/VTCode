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

        self.check_for_loops(tool_name)
    }

    fn check_for_loops(&mut self, tool_name: &str) -> Option<String> {
        let count = self.tool_counts.get(tool_name).copied().unwrap_or(0);

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
        let count = self.tool_counts.get(tool_name).copied().unwrap_or(0);
        let max_calls = self.get_limit_for_tool(tool_name);
        count >= max_calls * HARD_LIMIT_MULTIPLIER
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

            if suffix_names == prev_names && k >= 2 {
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
mod tests {
    #[test]
    fn matrix_actions_are_distinct_but_repeated_status_still_hits_the_limit() {
        let mut detector = LoopDetector::with_max_repeated_calls(2);
        for action in ["create", "start", "status"] {
            detector.record_call(tools::MATRIX, &serde_json::json!({"action":action,"matrix_id":"work"}));
            assert!(!detector.is_hard_limit_exceeded(tools::MATRIX));
        }
        assert!(
            detector
                .record_call(tools::MATRIX, &serde_json::json!({"action":"status","matrix_id":"work"}))
                .is_some()
        );
        assert!(detector.is_hard_limit_exceeded(tools::MATRIX));
    }
    use super::*;
    use serde_json::json;

    #[test]
    fn test_immediate_repetition_detection() {
        let mut detector = LoopDetector::with_max_repeated_calls(3);
        let args = json!({"path": "src/"});

        // First two calls - no warning
        assert!(detector.record_call(LEGACY_GREP_FILE, &args).is_none());
        assert!(detector.record_call(LEGACY_GREP_FILE, &args).is_none());

        // Third identical call - hard stop
        let warning = detector.record_call(LEGACY_GREP_FILE, &args);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains(HARD_STOP_PREFIX));
    }

    #[test]
    fn test_command_tools_skip_identical_hard_stop() {
        let mut detector = LoopDetector::new();
        let args = json!({"command": "cargo test"});

        assert!(detector.record_call(tools::RUN_PTY_CMD, &args).is_none());
        assert!(detector.record_call(tools::RUN_PTY_CMD, &args).is_none());
        assert!(detector.record_call(tools::RUN_PTY_CMD, &args).is_none());
    }

    #[test]
    fn test_exec_command_alias_skips_identical_hard_stop() {
        let mut detector = LoopDetector::new();
        let args = json!({"cmd": "cargo test"});

        assert!(detector.record_call(tools::EXEC_COMMAND, &args).is_none());
        assert!(detector.record_call(tools::EXEC_COMMAND, &args).is_none());
        assert!(detector.record_call(tools::EXEC_COMMAND, &args).is_none());
    }

    #[test]
    fn test_root_path_normalization() {
        let mut detector = LoopDetector::with_max_repeated_calls(3);

        // All these should be treated as identical
        let paths = [
            json!({"path": "."}),
            json!({"path": ""}),
            json!({"path": "././"}),
            json!({"path": "//"}),
            json!({}),
        ];

        for path in &paths[..2] {
            assert!(detector.record_call(LEGACY_LIST_FILES, path).is_none());
        }

        // Third call with any root variation should trigger
        let warning = detector.record_call(LEGACY_LIST_FILES, &paths[2]);
        assert!(warning.is_some());

        // Further root-only variations should continue to warn
        for path in &paths[3..] {
            assert!(detector.record_call(LEGACY_LIST_FILES, path).is_some());
        }
    }

    #[test]
    fn test_detects_repeated_calls() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let tool_name = "test_repeated_tool";
        detector.set_tool_limit(tool_name, MAX_READONLY_TOOL_CALLS);
        let args = json!({"path": "/src"});

        // Repetition heuristics (pattern detection and soft limits) should warn eventually.
        let mut saw_warning = false;
        for _ in 0..MAX_READONLY_TOOL_CALLS {
            if detector.record_call(tool_name, &args).is_some() {
                saw_warning = true;
            }
        }
        assert!(saw_warning);
        assert_eq!(detector.get_call_count(tool_name), MAX_READONLY_TOOL_CALLS);
    }

    #[test]
    fn test_hard_limit_enforcement() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let tool_name = "test_hard_limit_tool";
        detector.set_tool_limit(tool_name, 2);
        let args = json!({"pattern": "test"});

        // Hard limit is 2x configured soft limit.
        let hard_limit = 2 * HARD_LIMIT_MULTIPLIER;
        let mut saw_warning = false;
        for i in 0..hard_limit {
            let result = detector.record_call(tool_name, &args);
            if result.is_some() {
                saw_warning = true;
            }
            if i >= hard_limit - 1 {
                assert!(result.is_some());
            }
        }

        assert!(saw_warning);
        assert!(detector.is_hard_limit_exceeded(tool_name));
    }

    #[test]
    fn test_different_tools_no_warning() {
        let mut detector = LoopDetector::new();

        detector.record_call(LEGACY_LIST_FILES, &json!({"path": "/src"}));
        detector.record_call(LEGACY_GREP_FILE, &json!({"pattern": "test"}));
        detector.record_call(tools::READ_FILE, &json!({"path": "main.rs"}));

        assert_eq!(detector.tool_counts.len(), 3);
    }

    #[test]
    fn test_non_root_paths_distinct() {
        let mut detector = LoopDetector::new();

        // These should be treated as different calls
        detector.record_call(LEGACY_LIST_FILES, &json!({"path": "src"}));
        detector.record_call(LEGACY_LIST_FILES, &json!({"path": "docs"}));
        detector.record_call(LEGACY_LIST_FILES, &json!({"path": "tests"}));

        // Count for each should be 1
        assert_eq!(detector.tool_counts.get(LEGACY_LIST_FILES).copied().unwrap_or(0), 3);
    }

    #[test]
    fn test_identical_calls_trigger_hard_limit() {
        let mut detector = LoopDetector::with_max_repeated_calls(3);
        let args = json!({"path": "."});

        assert!(detector.record_call(tools::READ_FILE, &args).is_none());
        assert!(detector.record_call(tools::READ_FILE, &args).is_none());

        let warning = detector.record_call(tools::READ_FILE, &args);
        assert!(warning.is_some());
        assert!(detector.is_hard_limit_exceeded(tools::READ_FILE));
    }

    #[test]
    fn test_normalize_args_removes_pagination() {
        let args = json!({"path": "src", "page": 1, "per_page": 10});
        let normalized = normalize_args_for_detection(LEGACY_LIST_FILES, &args);

        assert!(normalized.get("page").is_none());
        assert!(normalized.get("per_page").is_none());
        assert_eq!(normalized.get("path").and_then(|v| v.as_str()), Some("src"));
    }

    #[test]
    fn test_reset_tool_clears_specific_tool() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let args = json!({"path": "src"});

        // Record calls for multiple tools
        detector.record_call(LEGACY_LIST_FILES, &args);
        detector.record_call(LEGACY_LIST_FILES, &args);
        detector.record_call(LEGACY_GREP_FILE, &json!({"pattern": "test"}));

        assert_eq!(detector.get_call_count(LEGACY_LIST_FILES), 2);
        assert_eq!(detector.get_call_count(LEGACY_GREP_FILE), 1);

        // Reset only list_files
        detector.reset_tool(LEGACY_LIST_FILES);

        assert_eq!(detector.get_call_count(LEGACY_LIST_FILES), 0);
        assert_eq!(detector.get_call_count(LEGACY_GREP_FILE), 1);
    }

    #[test]
    fn test_suggest_alternative_for_list_files() {
        let detector = LoopDetector::new();
        let suggestion = detector.suggest_alternative(LEGACY_LIST_FILES);

        assert!(suggestion.is_some());
        let msg = suggestion.unwrap();
        assert!(msg.contains("code_search"));
        assert!(msg.contains("file_types"));
        assert!(msg.contains("subdirectories"));
    }

    #[test]
    fn test_suggest_alternative_for_grep_file() {
        let detector = LoopDetector::new();
        let suggestion = detector.suggest_alternative(LEGACY_GREP_FILE);

        assert!(suggestion.is_some());
        let msg = suggestion.unwrap();
        assert!(msg.contains("exec_command.cmd"));
        assert!(msg.contains("definition or usage"));
        assert!(msg.contains("pattern"));
    }

    #[test]
    fn code_search_loop_fingerprint_uses_only_normalised_search_identity() {
        let first = json!({
            "query": " Widget ",
            "path": " src ",
            "file_types": [".rs", "python", "rust"],
            "result_types": ["path", "definition", "path"],
            "max_results": 5
        });
        let second = json!({
            "query": "Widget",
            "path": "src",
            "file_types": ["rust", ".py"],
            "result_types": ["definition", "path"],
            "max_results": 100
        });

        assert_eq!(hash_normalized_args(tools::CODE_SEARCH, &first), hash_normalized_args(tools::CODE_SEARCH, &second));

        for changed in [
            json!({"query": "Other", "path": "src", "file_types": ["rust", "python"], "result_types": ["definition", "path"]}),
            json!({"query": "Widget", "path": "tests", "file_types": ["rust", "python"], "result_types": ["definition", "path"]}),
            json!({"query": "Widget", "path": "src", "file_types": ["rust"], "result_types": ["definition", "path"]}),
            json!({"query": "Widget", "path": "src", "file_types": ["rust", "python"], "result_types": ["usage", "path"]}),
        ] {
            assert_ne!(
                hash_normalized_args(tools::CODE_SEARCH, &first),
                hash_normalized_args(tools::CODE_SEARCH, &changed)
            );
        }
    }

    #[test]
    fn test_suggest_alternative_for_unknown_tool() {
        let detector = LoopDetector::new();
        let suggestion = detector.suggest_alternative("unknown_tool");

        assert!(suggestion.is_some());
        let msg = suggestion.unwrap();
        assert!(msg.contains("root cause analysis"));
    }

    #[test]
    fn test_faster_detection_with_lower_limit() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        detector.set_tool_limit(LEGACY_LIST_FILES, 3);
        let args = json!({"path": "src"});

        // First call - no warning
        assert!(detector.record_call(LEGACY_LIST_FILES, &args).is_none());

        // Second call - no warning
        assert!(detector.record_call(LEGACY_LIST_FILES, &args).is_none());

        // Third call - should trigger warning (soft limit = 3)
        let warning = detector.record_call(LEGACY_LIST_FILES, &args);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("Loop detected"));
    }

    #[test]
    fn test_file_operation_action_suffix_uses_action_specific_limit() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let tool_key = format!("{}::read", tools::UNIFIED_FILE);

        for idx in 0..(MAX_WRITE_TOOL_CALLS * HARD_LIMIT_MULTIPLIER) {
            let args = json!({"path": "src/main.rs", "offset_lines": idx + 1, "limit": 1});
            let _ = detector.record_call(&tool_key, &args);
        }

        // Read action should not use write limits.
        assert!(!detector.is_hard_limit_exceeded(&tool_key));
    }

    #[test]
    fn test_file_operation_write_suffix_uses_write_limit() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let tool_key = format!("{}::write", tools::UNIFIED_FILE);

        for idx in 0..(MAX_WRITE_TOOL_CALLS * HARD_LIMIT_MULTIPLIER) {
            let args = json!({"path": format!("src/file_{idx}.rs"), "content": "x"});
            let _ = detector.record_call(&tool_key, &args);
        }

        assert!(detector.is_hard_limit_exceeded(&tool_key));
    }

    #[test]
    fn test_command_session_action_suffix_skips_identical_limit() {
        let mut detector = LoopDetector::with_max_repeated_calls(3);
        let tool_key = format!("{}::run", tools::UNIFIED_EXEC);
        let args = json!({"command": "cargo check"});

        assert!(detector.record_call(&tool_key, &args).is_none());
        assert!(detector.record_call(&tool_key, &args).is_none());
        assert!(detector.record_call(&tool_key, &args).is_none());
    }

    #[test]
    fn test_repetitive_read_target_with_small_variations_triggers_hard_stop() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let tool_key = format!("{}::read", tools::UNIFIED_FILE);
        let mut saw_hard_stop = false;

        for offset in [1, 2, 1, 2, 1, 2, 1, 2] {
            let args =
                json!({"path": "crates/codegen/vtcode-core/src/a2a/server.rs", "offset_lines": offset, "limit": 20});
            if let Some(warning) = detector.record_call(&tool_key, &args)
                && warning.contains(HARD_STOP_PREFIX)
            {
                saw_hard_stop = true;
            }
        }

        assert!(saw_hard_stop);
        assert!(detector.is_hard_limit_exceeded(&tool_key));
    }

    #[test]
    fn test_repetitive_read_target_with_many_ranges_is_not_hard_stopped() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let tool_key = format!("{}::read", tools::UNIFIED_FILE);

        for offset in 1..=MAX_SIMILAR_READ_TARGET_CALLS {
            let args = json!({"path": "crates/codegen/vtcode-core/src/a2a/server.rs", "offset_lines": offset * 40, "limit": 40});
            if let Some(warning) = detector.record_call(&tool_key, &args) {
                assert!(!warning.contains(HARD_STOP_PREFIX));
            }
        }

        assert!(!detector.is_hard_limit_exceeded(&tool_key));
    }

    #[test]
    fn test_repetitive_read_target_grep_calls_do_not_break_streak() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let read_tool = format!("{}::read", tools::UNIFIED_FILE);

        // Interleave grep calls between read_file calls on the same target.
        // With the new logic, grep (read-only) does not break the streak.
        // Use distinct offsets so variants stay above the threshold.
        // Limit iterations so the navigation streak stays below the hard stop (7).
        for offset in 1..=(MAX_SIMILAR_READ_TARGET_CALLS - 1) {
            let _ = detector.record_call(
                &read_tool,
                &json!({"path": "crates/codegen/vtcode-core/src/a2a/server.rs", "offset_lines": offset * 40, "limit": 20}),
            );
            let _ = detector.record_call(
                LEGACY_GREP_FILE,
                &json!({"pattern": "handle_loop_detection", "path": "crates/codegen/vtcode-core/src"}),
            );
        }

        // Variants exceed the threshold, so no repetitive-read-target hard stop.
        // But navigation hard stop may have fired (streak >= 7) -- check specifically
        // for the repetitive-read-target condition instead.
        assert!(!detector.is_hard_limit_exceeded(&read_tool));
    }

    #[test]
    fn test_repetitive_read_target_same_params_with_grep_between_triggers_hard_stop() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let read_tool = format!("{}::read", tools::UNIFIED_FILE);

        // Same offset repeated, with grep calls between reads.
        // Grep doesn't break the streak, so the hard stop fires.
        for _ in 0..MAX_SIMILAR_READ_TARGET_CALLS + 2 {
            let _ = detector.record_call(&read_tool, &json!({"path": "Cargo.lock", "offset_lines": 1, "limit": 2000}));
            let _ = detector.record_call(LEGACY_GREP_FILE, &json!({"pattern": "aws-lc", "path": "Cargo.lock"}));
        }

        assert!(detector.is_hard_limit_exceeded(&read_tool));
    }

    #[test]
    fn test_read_file_alias_cycling_triggers_identical_detection() {
        // Simulates the exact failure from the transcript: LLM cycles through
        // offset_lines, max_lines, chunk_lines, line_start/line_end, encoding
        // for the same file. Normalization should collapse them to identical hashes.
        let mut detector = LoopDetector::with_max_repeated_calls(3);

        let call1 = json!({"path": "docs/README.md", "max_lines": 200});
        let call2 = json!({"path": "docs/README.md", "offset_lines": 1, "limit": 200});
        let call3 = json!({"path": "docs/README.md", "chunk_lines": 200});

        // After normalization, all three should have: {path: "docs/README.md", offset: 1, limit: 200}
        let n1 = normalize_args_for_detection(tools::READ_FILE, &call1);
        let n2 = normalize_args_for_detection(tools::READ_FILE, &call2);
        let n3 = normalize_args_for_detection(tools::READ_FILE, &call3);

        // Verify aliases are normalized
        assert!(n1.get("max_lines").is_none(), "max_lines should be removed");
        assert!(n2.get("offset_lines").is_none(), "offset_lines should be removed");
        assert!(n3.get("chunk_lines").is_none(), "chunk_lines should be removed");
        assert_eq!(n1.get("limit"), n2.get("limit"));
        assert_eq!(n2.get("limit"), n3.get("limit"));

        // All three should trigger identical-call detection by call 3
        assert!(detector.record_call(tools::READ_FILE, &call1).is_none());
        assert!(detector.record_call(tools::READ_FILE, &call2).is_none());

        let warning = detector.record_call(tools::READ_FILE, &call3);
        assert!(warning.is_some(), "Third aliased call should be detected");
        assert!(warning.unwrap().contains(HARD_STOP_PREFIX));
    }

    #[test]
    fn test_read_file_encoding_and_action_are_stripped() {
        let with_encoding = json!({"path": "foo.rs", "encoding": "utf-8", "offset_lines": 1, "max_lines": 200});
        let without_encoding = json!({"path": "foo.rs", "offset_lines": 1, "max_lines": 200});

        let n1 = normalize_args_for_detection(tools::READ_FILE, &with_encoding);
        let n2 = normalize_args_for_detection(tools::READ_FILE, &without_encoding);

        assert!(n1.get("encoding").is_none());
        assert_eq!(n1, n2);
    }

    #[test]
    fn test_line_start_line_end_normalized_to_offset_limit() {
        let args = json!({"path": "foo.rs", "line_start": 1, "line_end": 200});
        let normalized = normalize_args_for_detection(tools::READ_FILE, &args);

        assert!(normalized.get("line_start").is_none());
        assert!(normalized.get("line_end").is_none());
        assert_eq!(normalized.get("offset").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(normalized.get("limit").and_then(|v| v.as_u64()), Some(200));
    }

    #[test]
    fn test_start_line_end_line_normalized_to_offset_limit() {
        let args = json!({"path": "Cargo.lock", "start_line": 550, "end_line": 590});
        let normalized = normalize_args_for_detection(tools::READ_FILE, &args);

        assert!(normalized.get("start_line").is_none());
        assert!(normalized.get("end_line").is_none());
        assert_eq!(normalized.get("offset").and_then(|v| v.as_u64()), Some(550));
        assert_eq!(normalized.get("limit").and_then(|v| v.as_u64()), Some(41));
    }

    #[test]
    fn test_navigation_loop_detection() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let list_args = serde_json::json!({"path": "src"});
        let grep_args = serde_json::json!({"pattern": "fn", "path": "src/main.rs"});
        let read_args = serde_json::json!({"path": "src/main.rs"});

        // Sequence: A, B, C (where A=LIST, B=GREP, C=READ)
        // 3 calls below the warning threshold of 4.
        let sequence = [
            (LEGACY_LIST_FILES, &list_args),
            (LEGACY_GREP_FILE, &grep_args),
            (tools::READ_FILE, &read_args),
        ];

        for (i, (tool, args)) in sequence.iter().enumerate() {
            let res = detector.record_call(tool, args);
            assert!(res.is_none(), "Call {} ({}) should not have triggered a warning", i + 1, tool);
        }

        // 4th call (any read-only) should trigger navigation loop warning (streak hits 4)
        let warning = detector.record_call(LEGACY_GREP_FILE, &grep_args);
        assert!(warning.is_some(), "4th call should have triggered a navigation loop warning");
        assert!(warning.unwrap().contains("Navigation loop detected"));

        // A mutating call should reset the streak
        let write_args = serde_json::json!({"path": "src/new.rs", "content": "test"});
        assert!(detector.record_call(tools::WRITE_FILE, &write_args).is_none());

        // Subsequent read calls should start from 0; single call should be fine
        assert!(detector.record_call(LEGACY_LIST_FILES, &list_args).is_none());
    }

    #[test]
    fn write_edit_repeat_on_same_path_is_not_identical_loop() {
        // Regression: two consecutive write_file calls to the same path with
        // DIFFERENT content must not trigger the identical-call HARD STOP.
        // The normalized hash nulls `content`, so without the write-tool
        // exemption every edit would hash identically and halt the task.
        let mut detector = LoopDetector::with_max_repeated_calls(2);
        let first = json!({"path": "src/main.rs", "content": "first draft"});
        let second = json!({"path": "src/main.rs", "content": "corrected"});

        assert!(detector.record_call(tools::WRITE_FILE, &first).is_none());
        let warning = detector.record_call(tools::WRITE_FILE, &second);
        assert!(warning.is_none(), "distinct edits to same path must not loop-stop: {warning:?}");
        assert!(!detector.is_hard_limit_exceeded(tools::WRITE_FILE));
    }

    #[test]
    fn apply_patch_repeat_on_same_file_is_not_identical_loop() {
        let mut detector = LoopDetector::with_max_repeated_calls(2);
        let first = json!({"path": "src/main.rs", "diff": "old A", "mode": "edit"});
        let second = json!({"path": "src/main.rs", "diff": "old B", "mode": "edit"});

        assert!(detector.record_call(tools::APPLY_PATCH, &first).is_none());
        assert!(detector.record_call(tools::APPLY_PATCH, &second).is_none());
        assert!(!detector.is_hard_limit_exceeded(tools::APPLY_PATCH));
    }

    #[test]
    fn exec_command_verification_read_uses_cmd_key() {
        // Regression: exec_command's schema uses the `cmd` key (not `command`).
        // A repeated `cat file` via exec_command must be detected as an
        // identical read command; a repeated `cargo check` must be exempt.
        let mut detector = LoopDetector::with_max_repeated_calls(2);
        let cat = json!({"cmd": "cat src/main.rs"});

        assert!(detector.record_call(tools::EXEC_COMMAND, &cat).is_none());
        let warning = detector.record_call(tools::EXEC_COMMAND, &cat);
        assert!(warning.is_some(), "repeated `cat file` via exec_command.cmd should hit the identical limit");

        let mut detector = LoopDetector::with_max_repeated_calls(2);
        let build = json!({"cmd": "cargo check"});
        assert!(detector.record_call(tools::EXEC_COMMAND, &build).is_none());
        assert!(
            detector.record_call(tools::EXEC_COMMAND, &build).is_none(),
            "repeated `cargo check` must be exempt from identical-call limits"
        );
    }

    #[test]
    fn exec_command_without_read_target_breaks_navigation_streak() {
        // Regression: `exec_command` (production shell tool) must reset the
        // readonly streak like other mutating tools, otherwise read -> build ->
        // read -> build hits the navigation HARD STOP.
        let mut detector = LoopDetector::with_max_repeated_calls(100);

        // 4 consecutive reads of DIFFERENT files => navigation warning fires at 4
        for i in 0..4 {
            let read_args = json!({"path": format!("src/file_{i}.rs")});
            let _ = detector.record_call(tools::READ_FILE, &read_args);
        }
        // A `cargo check` (no read target) breaks the streak
        assert!(
            detector
                .record_call(tools::EXEC_COMMAND, &json!({"cmd": "cargo check"}))
                .is_none()
        );
        // Reads after the break must not immediately hard-stop (streak restarted at 0)
        let read_args = json!({"path": "src/file_4.rs"});
        let warning = detector.record_call(tools::READ_FILE, &read_args);
        assert!(warning.is_none(), "read after mutating exec_command must not hit navigation stop: {warning:?}");
    }

    #[test]
    fn reset_clears_window_and_cache_order_state() {
        let mut detector = LoopDetector::with_max_repeated_calls(2);
        let read = json!({"path": "src/main.rs"});
        for _ in 0..4 {
            let _ = detector.record_call(tools::READ_FILE, &read);
        }
        detector.reset();
        assert_eq!(detector.norm_cache_order.len(), 0);
        assert_eq!(detector.action_hash_window.len(), 0);
        assert_eq!(detector.tool_counts.len(), 0);
    }

    #[test]
    fn test_checkpoint_324_pattern_detected() {
        // Simulates the exact failure from turn_324 checkpoint:
        // Agent reads Cargo.lock with start_line/end_line (ignored), retries with
        // different values, interleaves grep calls. With lowered navigation
        // thresholds (streak >= 4 warns, >= 7 hard-stops), the navigation loop
        // warning fires at call 4 and the repetitive-read-target hard stop
        // fires at call 6.
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let read_tool = format!("{}::read", tools::UNIFIED_FILE);

        // Call 1: read Cargo.toml (different target)
        let r = detector.record_call(&read_tool, &json!({"path": "Cargo.toml"}));
        assert!(r.is_none());

        // Call 2: read Cargo.lock
        let r = detector.record_call(&read_tool, &json!({"path": "Cargo.lock"}));
        assert!(r.is_none());

        // Call 3: read Cargo.lock (streak=2)
        let r = detector.record_call(&read_tool, &json!({"path": "Cargo.lock"}));
        assert!(r.is_none());

        // Call 4: grep Cargo.lock (read-only, does NOT break streak, streak=4)
        // Navigation loop warning fires at streak 4 with the lowered threshold.
        let r = detector.record_call(LEGACY_GREP_FILE, &json!({"pattern": "aws-lc", "path": "Cargo.lock"}));
        assert!(r.is_some(), "Navigation loop warning should fire at streak 4");
        let msg = r.unwrap();
        assert!(msg.contains("Navigation loop detected"));

        // Call 5: read Cargo.lock with start_line (streak=5)
        // Cooldown suppresses the navigation warning; no repetitive-read warning yet.
        let r = detector.record_call(&read_tool, &json!({"path": "Cargo.lock", "start_line": 550, "end_line": 590}));
        assert!(r.is_none());

        // Call 6: read Cargo.lock with different start_line (streak=6, variants=3)
        // Repetitive-read-target HARD STOP fires: same_target_streak >= 4 && variants <= 3
        let r = detector.record_call(&read_tool, &json!({"path": "Cargo.lock", "start_line": 4400, "end_line": 4420}));
        assert!(r.is_some(), "HARD STOP should fire at call 6");
        let msg = r.unwrap();
        assert!(msg.contains(HARD_STOP_PREFIX), "Expected hard stop, got: {msg}");
        assert!(msg.contains("Cargo.lock"));
        assert!(msg.contains("exact continuation range"));
        assert!(detector.is_hard_limit_exceeded(&read_tool));
    }

    // --- file_operation read-action tests ---

    #[test]
    fn read_target_extracted_for_file_operation_read_action() {
        let args = json!({"action": "read", "path": "src/lib.rs"});
        let target = read_target_for_tool_call(tools::UNIFIED_FILE, &args);
        assert_eq!(target.as_deref(), Some("src/lib.rs"));
    }

    #[test]
    fn read_target_none_for_file_operation_write_action() {
        let args = json!({"action": "write", "path": "src/lib.rs", "content": "fn main() {}"});
        let target = read_target_for_tool_call(tools::UNIFIED_FILE, &args);
        assert!(target.is_none());
    }

    #[test]
    fn read_target_extracted_for_file_operation_with_read_suffix() {
        let args = json!({"path": "src/main.rs"});
        let read_tool = format!("{}::read", tools::UNIFIED_FILE);
        let target = read_target_for_tool_call(&read_tool, &args);
        assert_eq!(target.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn repetitive_read_target_fires_for_file_operation_read_action() {
        let mut detector = LoopDetector::new();

        // Use only 2 different offsets so variants stays <= MAX_SIMILAR_READ_TARGET_VARIANTS.
        // With 4 calls to the same file, same_target_streak >= MAX_SIMILAR_READ_TARGET_CALLS
        // and variants <= MAX_SIMILAR_READ_TARGET_VARIANTS => HARD STOP fires.
        let offsets = [0, 100, 0, 100];
        for (i, offset) in offsets.iter().enumerate() {
            let result = detector
                .record_call(tools::UNIFIED_FILE, &json!({"action": "read", "path": "src/lib.rs", "offset": offset}));
            if i < offsets.len() - 1 {
                assert!(result.is_none(), "call {i} should not trigger");
            } else {
                assert!(result.is_some(), "call {i} should trigger HARD STOP");
                let msg = result.unwrap();
                assert!(msg.contains(HARD_STOP_PREFIX), "Expected hard stop: {msg}");
                assert!(msg.contains("src/lib.rs"));
            }
        }
    }

    // --- Global readonly budget tests ---

    #[test]
    fn global_readonly_budget_fires_at_limit() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let mut saw_hard_stop = false;

        // Make MAX_TOTAL_READONLY_CALLS calls with different args to avoid per-tool detection
        for i in 0..MAX_TOTAL_READONLY_CALLS {
            let args = json!({"query": format!("pattern_{i}"), "path": "src/"});
            if let Some(msg) = detector.record_call(tools::CODE_SEARCH, &args)
                && msg.contains("global read-only budget")
            {
                saw_hard_stop = true;
                break;
            }
        }

        assert!(saw_hard_stop, "Global readonly budget should fire at {MAX_TOTAL_READONLY_CALLS}");
        assert!(detector.is_hard_limit_exceeded(tools::CODE_SEARCH));
    }

    #[test]
    fn global_readonly_budget_counts_across_different_tools() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let mut hard_stop_count = 0;

        // Alternate between code_search and file_operation to evade per-tool limits.
        for i in 0..MAX_TOTAL_READONLY_CALLS + 5 {
            if i % 2 == 0 {
                let args = json!({"query": format!("p_{i}"), "path": "src/"});
                if let Some(msg) = detector.record_call(tools::CODE_SEARCH, &args)
                    && msg.contains("global read-only budget")
                {
                    hard_stop_count += 1;
                }
            } else {
                let args = json!({"action": "read", "path": format!("src/file_{i}.rs")});
                if let Some(msg) = detector.record_call(tools::UNIFIED_FILE, &args)
                    && msg.contains("global read-only budget")
                {
                    hard_stop_count += 1;
                }
            }
        }

        assert!(hard_stop_count > 0, "Global budget should fire when alternating tools");
        assert_eq!(detector.total_readonly_calls(), MAX_TOTAL_READONLY_CALLS + 5);
    }

    #[test]
    fn exhausted_global_readonly_budget_blocks_every_tool_and_says_so() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        for i in 0..MAX_TOTAL_READONLY_CALLS {
            let args = json!({"query": format!("p_{i}"), "path": "src/"});
            detector.record_call(tools::CODE_SEARCH, &args);
        }

        let edit = json!({"path": "src/new.rs", "content": "fn main() {}"});
        let msg = detector
            .record_call(tools::WRITE_FILE, &edit)
            .expect("an edit after the budget is exhausted is also stopped");
        assert!(msg.starts_with(HARD_STOP_PREFIX), "{msg}");
        assert!(
            msg.contains("so every further tool call this turn is blocked, including edits and commands."),
            "{msg}"
        );
        assert!(!msg.contains("further read-only calls are blocked"), "{msg}");
    }

    #[test]
    fn global_readonly_budget_resets_on_reset() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);

        // Make some readonly calls
        for i in 0..10 {
            let args = json!({"query": format!("p_{i}"), "path": "src/"});
            detector.record_call(tools::CODE_SEARCH, &args);
        }
        assert_eq!(detector.total_readonly_calls(), 10);

        detector.reset();
        assert_eq!(detector.total_readonly_calls(), 0);
    }

    #[test]
    fn global_readonly_budget_not_incremented_by_mutating_tools() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);

        // Make mutating calls
        for i in 0..5 {
            let args = json!({"path": format!("src/file_{i}.rs"), "content": "fn main() {}"});
            detector.record_call(tools::WRITE_FILE, &args);
        }

        assert_eq!(detector.total_readonly_calls(), 0);
    }

    #[test]
    fn lowered_navigation_loop_thresholds() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);

        // Make 3 read-only calls (below threshold)
        for i in 0..3 {
            let args = json!({"query": format!("p_{i}"), "path": "src/"});
            assert!(detector.record_call(tools::CODE_SEARCH, &args).is_none(), "Call {i} should not trigger warning");
        }

        // 4th call should trigger navigation loop warning (streak hits 4)
        let args = json!({"query": "p_4", "path": "src/"});
        let warning = detector.record_call(tools::CODE_SEARCH, &args);
        assert!(warning.is_some(), "Navigation loop warning should fire at streak 4");
        assert!(warning.unwrap().contains("Navigation loop detected"));
    }

    #[test]
    fn navigation_hard_stop_at_lowered_threshold() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);

        // Make 7 read-only calls (new hard stop threshold)
        for i in 0..6 {
            let args = json!({"query": format!("p_{i}"), "path": "src/"});
            detector.record_call(tools::CODE_SEARCH, &args);
        }

        // 7th call should trigger HARD STOP
        let args = json!({"query": "p_7", "path": "src/"});
        let warning = detector.record_call(tools::CODE_SEARCH, &args);
        assert!(warning.is_some(), "Navigation hard stop should fire at streak 7");
        let msg = warning.unwrap();
        assert!(msg.contains(HARD_STOP_PREFIX), "Expected hard stop: {msg}");
        assert!(detector.is_hard_limit_exceeded(tools::CODE_SEARCH));
    }

    #[test]
    fn subagent_navigation_hard_stop_at_5() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        detector.set_subagent_mode(true);

        // Make 4 read-only calls (subagent warning at 3, but no hard stop yet)
        for i in 0..4 {
            let args = json!({"query": format!("p_{i}"), "path": "src/"});
            detector.record_call(tools::CODE_SEARCH, &args);
        }

        // 5th call should trigger HARD STOP for subagent
        let args = json!({"query": "p_5", "path": "src/"});
        let warning = detector.record_call(tools::CODE_SEARCH, &args);
        assert!(warning.is_some(), "Subagent navigation hard stop should fire at streak 5");
        let msg = warning.unwrap();
        assert!(msg.contains(HARD_STOP_PREFIX), "Expected hard stop: {msg}");
    }

    #[test]
    fn subagent_readonly_budget_at_20() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        detector.set_subagent_mode(true);

        // Make 20 read-only calls with different args to avoid per-tool detection
        for i in 0..20 {
            let args = json!({"query": format!("unique_{i}"), "path": "src/"});
            detector.record_call(tools::CODE_SEARCH, &args);
        }

        // 21st call should trigger global budget HARD STOP
        let args = json!({"query": "final", "path": "src/"});
        let warning = detector.record_call(tools::CODE_SEARCH, &args);
        assert!(warning.is_some(), "Subagent global read-only budget should fire at 20");
        let msg = warning.unwrap();
        assert!(msg.contains("global read-only budget is exhausted"), "Expected budget exhaustion: {msg}");
        assert!(msg.contains("limit: 20"), "Expected limit 20 in message: {msg}");
    }

    #[test]
    fn main_agent_readonly_budget_still_30() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        // NOT setting subagent mode -- default is false

        // Make 30 read-only calls
        for i in 0..30 {
            let args = json!({"query": format!("unique_{i}"), "path": "src/"});
            detector.record_call(tools::CODE_SEARCH, &args);
        }

        // 31st call should trigger global budget HARD STOP
        let args = json!({"query": "final", "path": "src/"});
        let warning = detector.record_call(tools::CODE_SEARCH, &args);
        assert!(warning.is_some(), "Main agent global read-only budget should fire at 30");
        let msg = warning.unwrap();
        assert!(msg.contains("limit: 30"), "Expected limit 30 in message: {msg}");
    }

    // --- canonicalize_command_for_detection tests ---

    #[test]
    fn canonicalize_command_v_to_verify() {
        assert_eq!(canonicalize_command_for_detection("command -v ast-grep"), Some("__verify__:ast-grep".to_string()));
    }

    #[test]
    fn canonicalize_which_to_verify() {
        assert_eq!(canonicalize_command_for_detection("which ast-grep"), Some("__verify__:ast-grep".to_string()));
    }

    #[test]
    fn canonicalize_help_to_verify() {
        assert_eq!(canonicalize_command_for_detection("ast-grep --help"), Some("__verify__:ast-grep".to_string()));
    }

    #[test]
    fn canonicalize_version_to_verify() {
        assert_eq!(canonicalize_command_for_detection("ast-grep --version"), Some("__verify__:ast-grep".to_string()));
    }

    #[test]
    fn canonicalize_short_help_to_verify() {
        assert_eq!(canonicalize_command_for_detection("ast-grep -h"), Some("__verify__:ast-grep".to_string()));
    }

    #[test]
    fn canonicalize_path_prefix_stripped() {
        assert_eq!(
            canonicalize_command_for_detection("command -v /usr/bin/ast-grep"),
            Some("__verify__:ast-grep".to_string())
        );
    }

    #[test]
    fn canonicalize_cat_to_read() {
        assert_eq!(canonicalize_command_for_detection("cat src/main.rs"), Some("__read__:src/main.rs".to_string()));
    }

    #[test]
    fn canonicalize_head_to_read() {
        assert_eq!(canonicalize_command_for_detection("head src/main.rs"), Some("__read__:src/main.rs".to_string()));
    }

    #[test]
    fn canonicalize_arbitrary_command_returns_none() {
        assert_eq!(canonicalize_command_for_detection("cargo check -p vtcode-core"), None);
    }

    #[test]
    fn canonicalize_empty_command_returns_none() {
        assert_eq!(canonicalize_command_for_detection(""), None);
    }

    #[test]
    fn canonicalize_env_prefix_skips_to_real_tool() {
        assert_eq!(
            canonicalize_command_for_detection("env VAR=val ast-grep --help"),
            Some("__verify__:ast-grep".to_string())
        );
    }

    #[test]
    fn canonicalize_env_prefix_with_path() {
        assert_eq!(
            canonicalize_command_for_detection("env PATH=/usr/bin ast-grep --version"),
            Some("__verify__:ast-grep".to_string())
        );
    }

    // --- Command normalization integration tests ---

    #[test]
    fn verification_commands_normalize_to_same_hash() {
        let mut detector = LoopDetector::with_max_repeated_calls(2);
        let tool = tools::UNIFIED_EXEC;

        // These should all normalize to __verify__:ast-grep
        let args1 = json!({"command": "command -v ast-grep"});
        let args2 = json!({"command": "which ast-grep"});
        let _args3 = json!({"command": "ast-grep --help"});

        assert!(detector.record_call(tool, &args1).is_none());
        // Second call with equivalent command should trigger
        let warning = detector.record_call(tool, &args2);
        assert!(warning.is_some(), "which ast-grep should be detected as duplicate of command -v ast-grep");
    }

    #[test]
    fn cat_commands_normalize_to_read_target() {
        let mut detector = LoopDetector::with_max_repeated_calls(100);
        let tool = tools::UNIFIED_EXEC;

        // cat file should set read_target
        let args = json!({"command": "cat src/main.rs"});
        detector.record_call(tool, &args);

        // Check that the last record has a read_target
        let record = detector.recent_calls.back().unwrap();
        assert_eq!(record.read_target.as_deref(), Some("src/main.rs"), "cat command should produce read_target");
    }

    #[test]
    fn verification_spiral_detected_across_tools() {
        let mut detector = LoopDetector::with_max_repeated_calls(2);
        let tool = tools::UNIFIED_EXEC;

        // Verification spiral: different commands checking the same tool
        let args1 = json!({"command": "command -v ast-grep"});
        let _args2 = json!({"command": "ast-grep --version 2>&1 | head -5"});

        assert!(detector.record_call(tool, &args1).is_none());
        // --version doesn't normalize (has pipe), so this shouldn't trigger
        // via identical-call detection. But the first two `command -v` variants
        // should be caught.
    }
}
