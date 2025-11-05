// EXPERIMENTAL FEATURE: Smart Conversation Summarization
//
// This module provides intelligent conversation compression and summarization for long-running sessions.
// It's designed to prevent context window overflow by selectively compressing less important turns
// while preserving critical information.
//
// ## Status: EXPERIMENTAL - Disabled by Default
//
// This feature is experimental and may affect conversation quality. It is disabled by default.
//
// ## Configuration
//
// Enable via environment variables or vtcode.toml:
//
// ### Environment Variables:
// - `VTCODE_SMART_SUMMARIZATION_ENABLED=true` - Enable the feature (default: false)
// - `VTCODE_SMART_SUMMARIZATION_INTERVAL=30` - Min seconds between summarizations (default: 30)
// - `VTCODE_SMART_SUMMARIZATION_MAX_CONCURRENT=4` - Max concurrent tasks (default: 4)
// - `VTCODE_SMART_SUMMARIZATION_MAX_TURN_LENGTH=2000` - Max chars per turn (default: 2000)
// - `VTCODE_SMART_SUMMARIZATION_AGGRESSIVE_THRESHOLD=15000` - Aggressive compression threshold (default: 15000)
//
// ### vtcode.toml:
// ```toml
// [agent.smart_summarization]
// enabled = false  # Experimental feature, disabled by default
// min_summary_interval_secs = 30
// max_concurrent_tasks = 4
// min_turns_threshold = 20
// token_threshold_percent = 0.6
// max_turn_content_length = 2000
// aggressive_compression_threshold = 15000
// ```
//
// ## Features
//
// 1. **Rule-Based Compression**:
//    - Importance scoring for each turn (position, role, keywords, task presence)
//    - Semantic similarity detection using Jaccard similarity
//    - Extractive summarization for long messages
//    - Intelligent filtering of low-value content
//
// 2. **Advanced Error Pattern Analysis**:
//    - Temporal clustering detection
//    - Recovery rate analysis
//    - Root cause identification
//    - Context-aware recommendations
//
// 3. **Comprehensive Summary Generation**:
//    - Task completion tracking
//    - Decision history extraction
//    - Error pattern analysis with solutions
//    - Compression metrics and confidence scoring
//
// ## Algorithm Overview
//
// 1. Score importance of each conversation turn
// 2. Remove redundant system messages (keep first, last, and error-related)
// 3. Detect and skip near-duplicate consecutive messages (>90% similarity)
// 4. Intelligently truncate long messages using extractive summarization
// 5. Apply additional compression if total size exceeds threshold
// 6. Generate final summary with tasks, decisions, errors, and recommendations
//
// ## Performance
//
// - Memory efficient: O(n) space complexity
// - Scalable: O(n log n) time complexity for most operations
// - Non-blocking: Async-ready for LLM integration
//

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, error};
use uuid::Uuid;
use vtcode_core::core::conversation_summarizer::{
    ConversationSummary, ConversationTurn, DecisionInfo, ErrorInfo,
};

// Configuration constants - these can be overridden via environment variables
// EXPERIMENTAL FEATURE: Smart summarization is disabled by default
const DEFAULT_ENABLED: bool = false;
const DEFAULT_MIN_SUMMARY_INTERVAL_SECS: u64 = 30;
const DEFAULT_MAX_CONCURRENT_TASKS: usize = 4;
const DEFAULT_MAX_TURN_CONTENT_LENGTH: usize = 2000;
const DEFAULT_AGGRESSIVE_COMPRESSION_THRESHOLD: usize = 15_000;

/// Priority levels for summarization tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub enum SummarizationPriority {
    High = 3,   // Critical system operations
    Medium = 2, // User-requested or important operations
    Low = 1,    // Background maintenance
}

/// A summarization task to be processed
#[allow(dead_code)]
struct SummarizationTask {
    conversation_turns: Vec<ConversationTurn>,
    decision_history: Vec<DecisionInfo>,
    error_history: Vec<ErrorInfo>,
    session_start_time: u64,
    priority: SummarizationPriority,
    callback: Option<Box<dyn FnOnce(Result<ConversationSummary, String>) + Send>>,
}

/// The smart summarizer that handles background summarization
#[allow(dead_code)]
pub struct SmartSummarizer {
    sender: mpsc::Sender<SummarizationTask>,
    worker_handle: Option<JoinHandle<()>>,
    is_running: Arc<AtomicBool>,
    is_enabled: bool, // Track if feature is enabled
    last_summary_size: Arc<AtomicU64>,
    last_summary_time: Arc<Mutex<Instant>>,
    min_summary_interval: Duration,
    max_concurrent_tasks: usize,
}

#[allow(dead_code)]
impl SmartSummarizer {
    /// Create a new SmartSummarizer with default settings
    ///
    /// EXPERIMENTAL: This feature is disabled by default. Set environment variable
    /// VTCODE_SMART_SUMMARIZATION_ENABLED=true to enable it.
    pub fn new() -> Self {
        let enabled = std::env::var("VTCODE_SMART_SUMMARIZATION_ENABLED")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(DEFAULT_ENABLED);

        if !enabled {
            debug!(
                "Smart summarization is DISABLED (experimental feature). Set VTCODE_SMART_SUMMARIZATION_ENABLED=true to enable."
            );
        } else {
            debug!("Smart summarization is ENABLED (experimental feature)");
        }

        let min_interval_secs = std::env::var("VTCODE_SMART_SUMMARIZATION_INTERVAL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MIN_SUMMARY_INTERVAL_SECS);

        let max_concurrent = std::env::var("VTCODE_SMART_SUMMARIZATION_MAX_CONCURRENT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_CONCURRENT_TASKS);

        Self::with_options(
            enabled,
            Duration::from_secs(min_interval_secs),
            max_concurrent,
        )
    }

    /// Create a new SmartSummarizer with custom options
    pub fn with_options(
        is_enabled: bool,
        min_summary_interval: Duration,
        max_concurrent_tasks: usize,
    ) -> Self {
        let (sender, mut receiver) = mpsc::channel::<SummarizationTask>(32); // Buffer up to 32 tasks
        let is_running = Arc::new(AtomicBool::new(true));
        let last_summary_size = Arc::new(AtomicU64::new(0));
        let last_summary_time = Arc::new(Mutex::new(Instant::now()));

        let worker_is_running = is_running.clone();
        let worker_last_summary_size = last_summary_size.clone();
        let worker_last_summary_time = last_summary_time.clone();

        // Spawn the worker task
        let worker_handle = tokio::spawn(async move {
            let mut task_queue: VecDeque<SummarizationTask> = VecDeque::new();
            let mut current_tasks = Vec::with_capacity(max_concurrent_tasks);
            let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent_tasks));

            while worker_is_running.load(Ordering::SeqCst) || !task_queue.is_empty() {
                // Process completed tasks
                current_tasks.retain_mut(|task: &mut SummarizationTaskHandle| {
                    if let Some((result, _)) = task.take_result() {
                        if let (Some(callback), result) = (task.callback.take(), result) {
                            callback(result);
                        }
                        false
                    } else {
                        true
                    }
                });

                // Try to start new tasks if we have capacity
                while current_tasks.len() < max_concurrent_tasks {
                    // Get the highest priority task
                    let task_info = task_queue
                        .iter()
                        .enumerate()
                        .max_by_key(|(_, t)| t.priority)
                        .map(|(idx, _)| idx);

                    if let Some(pos) = task_info {
                        let task = task_queue.remove(pos).unwrap();
                        let permit = semaphore.clone().acquire_owned().await.unwrap();

                        let task_handle = tokio::spawn({
                            let last_summary_size = worker_last_summary_size.clone();
                            let last_summary_time = worker_last_summary_time.clone();

                            async move {
                                let _start_time = Instant::now();
                                let result = process_task(task).await;

                                // Update metrics
                                if let Ok(ref summary) = result {
                                    last_summary_size
                                        .store(summary.summary_text.len() as u64, Ordering::SeqCst);
                                    *last_summary_time.lock().await = Instant::now();
                                }

                                (result, permit)
                            }
                        });

                        current_tasks.push(SummarizationTaskHandle {
                            handle: task_handle,
                            callback: None,
                        });
                        continue;
                    }
                    break;
                }

                // Wait for either a new task or a task completion
                tokio::select! {
                    // Try to receive a new task
                    Some(mut task) = receiver.recv() => {
                        if current_tasks.len() < max_concurrent_tasks {
                            let permit = semaphore.clone().acquire_owned().await.unwrap();
                            let last_summary_size = worker_last_summary_size.clone();
                            let last_summary_time = worker_last_summary_time.clone();
                            let callback = task.callback.take();

                            let handle = tokio::spawn(async move {
                                let _start_time = Instant::now();
                                let result = process_task(task).await;

                                // Update metrics
                                if let Ok(ref summary) = result {
                                    last_summary_size.store(
                                        summary.summary_text.len() as u64,
                                        Ordering::SeqCst,
                                    );
                                    *last_summary_time.lock().await = Instant::now();
                                }

                                (result, permit)
                            });

                            current_tasks.push(SummarizationTaskHandle {
                                handle,
                                callback,
                            });
                        } else {
                            task_queue.push_back(task);
                        }
                    },
                    // Poll tasks for completion (handled in retain_mut above)
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        // Just wake up periodically to check completed tasks
                    }
                }
            }
        });

        Self {
            sender,
            worker_handle: Some(worker_handle),
            is_running,
            is_enabled,
            last_summary_size,
            last_summary_time,
            min_summary_interval,
            max_concurrent_tasks,
        }
    }

    /// Check if the smart summarization feature is enabled
    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    /// Check if we should trigger summarization based on token usage
    pub async fn should_trigger_summarization(
        &self,
        current_tokens: usize,
        max_tokens: usize,
        min_turns_since_last: usize,
        turns_since_last: usize,
    ) -> bool {
        // Feature must be enabled
        if !self.is_enabled {
            return false;
        }

        // Check token threshold (60% of max)
        let token_threshold = (max_tokens as f64 * 0.6) as usize;
        let token_trigger = current_tokens >= token_threshold;

        // Check time since last summary
        let time_since_last = self.last_summary_time.lock().await.elapsed();
        let time_trigger = time_since_last >= self.min_summary_interval;

        // Check minimum turns since last summary
        let turns_trigger = turns_since_last >= min_turns_since_last;

        token_trigger && (time_trigger || turns_trigger)
    }

    /// Submit a summarization task
    pub async fn summarize(
        &self,
        conversation_turns: Vec<ConversationTurn>,
        decision_history: Vec<DecisionInfo>,
        error_history: Vec<ErrorInfo>,
        session_start_time: u64,
        priority: SummarizationPriority,
        callback: Option<Box<dyn FnOnce(Result<ConversationSummary, String>) + Send>>,
    ) -> Result<(), String> {
        // Feature must be enabled
        if !self.is_enabled {
            return Err("Smart summarization is disabled. Enable with VTCODE_SMART_SUMMARIZATION_ENABLED=true".to_string());
        }
        let task = SummarizationTask {
            conversation_turns,
            decision_history,
            error_history,
            session_start_time,
            priority,
            callback,
        };

        self.sender
            .send(task)
            .await
            .map_err(|e| format!("Failed to queue summarization task: {}", e))
    }

    /// Get the size of the last summary in bytes
    pub fn last_summary_size(&self) -> u64 {
        self.last_summary_size.load(Ordering::Relaxed)
    }

    /// Get the time since the last summary was completed
    pub async fn time_since_last_summary(&self) -> Duration {
        self.last_summary_time.lock().await.elapsed()
    }
}

impl Drop for SmartSummarizer {
    fn drop(&mut self) {
        // Signal the worker to stop
        self.is_running.store(false, Ordering::SeqCst);

        // Wait for the worker to finish
        if let Some(handle) = self.worker_handle.take() {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    if let Err(e) = handle.await {
                        error!("Error in summarization worker: {}", e);
                    }
                });
            });
        }
    }
}

// Helper types and functions

#[allow(dead_code)]
struct SummarizationTaskHandle {
    handle: JoinHandle<(
        Result<ConversationSummary, String>,
        tokio::sync::OwnedSemaphorePermit,
    )>,
    callback: Option<Box<dyn FnOnce(Result<ConversationSummary, String>) + Send>>,
}

#[allow(dead_code)]
impl SummarizationTaskHandle {
    fn take_result(
        &mut self,
    ) -> Option<(
        Result<ConversationSummary, String>,
        Option<Box<dyn FnOnce(Result<ConversationSummary, String>) + Send>>,
    )> {
        if self.handle.is_finished() {
            match tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(&mut self.handle)
            }) {
                Ok((result, _permit)) => Some((result, self.callback.take())),
                Err(e) => Some((
                    Err(format!("Summarization task panicked: {}", e)),
                    self.callback.take(),
                )),
            }
        } else {
            None
        }
    }
}

#[allow(dead_code)]
async fn process_task(task: SummarizationTask) -> Result<ConversationSummary, String> {
    // First pass: Quick rule-based compression
    let compressed_turns = compress_conversation(&task.conversation_turns);

    // If we still have too much data, do a second pass with LLM
    let final_turns = if needs_llm_compression(&compressed_turns) {
        compress_with_llm(compressed_turns).await?
    } else {
        compressed_turns
    };

    // Generate the final summary
    generate_final_summary(
        &final_turns,
        &task.decision_history,
        &task.error_history,
        task.session_start_time,
    )
    .await
}

#[allow(dead_code)]
fn compress_conversation(turns: &[ConversationTurn]) -> Vec<ConversationTurn> {
    if turns.is_empty() {
        return vec![];
    }

    // Load compression parameters from environment or use defaults
    let max_content_length = std::env::var("VTCODE_SMART_SUMMARIZATION_MAX_TURN_LENGTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_TURN_CONTENT_LENGTH);

    let aggressive_threshold = std::env::var("VTCODE_SMART_SUMMARIZATION_AGGRESSIVE_THRESHOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_AGGRESSIVE_COMPRESSION_THRESHOLD);

    let mut compressed = Vec::with_capacity(turns.len()); // Score importance of each turn for better compression decisions
    let importance_scores: Vec<f64> = turns
        .iter()
        .enumerate()
        .map(|(i, turn)| calculate_turn_importance(turn, i, turns.len()))
        .collect();

    for (i, turn) in turns.iter().enumerate() {
        let mut compressed_turn = turn.clone();
        let importance = importance_scores[i];

        // Skip redundant system messages (keep first, last, and important ones)
        if turn.role == "system" {
            if i == 0 || i == turns.len() - 1 || importance > 0.7 {
                // Keep first, last, and high-importance system messages
                compressed.push(compressed_turn);
            } else if turn.content.contains("error")
                || turn.content.contains("Error")
                || turn.content.contains("warning")
                || turn.content.contains("critical")
            {
                // Keep error-related system messages
                compressed.push(compressed_turn);
            }
            continue;
        }

        // Check for duplicate or near-duplicate consecutive messages
        if let Some(last) = compressed.last() {
            if last.role == turn.role {
                // Calculate similarity using simple word overlap
                let similarity = calculate_text_similarity(&last.content, &turn.content);

                if similarity > 0.9 {
                    // Skip near-duplicate messages
                    continue;
                }

                // Merge short consecutive assistant messages
                if last.role == "assistant"
                    && turn.role == "assistant"
                    && last.content.len() < 200
                    && turn.content.len() < 200
                    && similarity < 0.5
                // Only if not too similar
                {
                    if let Some(last_mut) = compressed.last_mut() {
                        last_mut.content = format!("{}\n\n{}", last_mut.content, turn.content);
                    }
                    continue;
                }
            }
        }

        // Truncate long messages intelligently based on importance
        if turn.content.len() > max_content_length {
            let truncated = if importance > 0.6 {
                // High importance: keep more context with smart extraction
                extract_important_segments(&turn.content, max_content_length)
            } else {
                // Lower importance: simple truncation with beginning/end
                let start = &turn.content[..max_content_length / 2];
                let end_start = turn.content.len().saturating_sub(max_content_length / 2);
                let end = &turn.content[end_start..];
                format!(
                    "{}\n... [truncated {} chars] ...\n{}",
                    start,
                    turn.content.len() - max_content_length,
                    end
                )
            };
            compressed_turn.content = truncated;
        }

        compressed.push(compressed_turn);
    }

    // Post-processing: Remove low-value filler content if still too large
    let total_size: usize = compressed.iter().map(|t| t.content.len()).sum();
    if total_size > aggressive_threshold {
        compressed = remove_low_value_turns(compressed, &importance_scores);
    }

    compressed
}

/// Calculate importance score for a conversation turn (0.0 - 1.0)
fn calculate_turn_importance(turn: &ConversationTurn, position: usize, total: usize) -> f64 {
    let mut score = 0.0;

    // Position-based importance (first and last are more important)
    let position_factor = if position == 0 || position == total - 1 {
        0.3
    } else if position < total / 4 || position > (3 * total) / 4 {
        0.2 // First and last quarter
    } else {
        0.1
    };
    score += position_factor;

    // Role-based importance
    let role_factor = match turn.role.as_str() {
        "user" => 0.3,      // User queries are important
        "assistant" => 0.2, // Assistant responses
        "system" => 0.1,    // System messages less important
        "tool" => 0.2,      // Tool results are relevant
        _ => 0.1,
    };
    score += role_factor;

    // Content-based importance (keywords indicating significance)
    let important_keywords = [
        "error",
        "warning",
        "critical",
        "failed",
        "success",
        "completed",
        "decision",
        "change",
        "update",
        "fix",
        "bug",
        "issue",
    ];
    let content_lower = turn.content.to_lowercase();
    let keyword_count = important_keywords
        .iter()
        .filter(|&kw| content_lower.contains(kw))
        .count();
    score += (keyword_count as f64 * 0.05).min(0.2);

    // Task info presence
    if turn.task_info.is_some() {
        score += 0.2;
    }

    score.min(1.0)
}

/// Calculate text similarity using word overlap (Jaccard similarity)
fn calculate_text_similarity(text1: &str, text2: &str) -> f64 {
    use std::collections::HashSet;

    let words1: HashSet<&str> = text1.split_whitespace().collect();
    let words2: HashSet<&str> = text2.split_whitespace().collect();

    if words1.is_empty() && words2.is_empty() {
        return 1.0;
    }
    if words1.is_empty() || words2.is_empty() {
        return 0.0;
    }

    let intersection = words1.intersection(&words2).count();
    let union = words1.union(&words2).count();

    intersection as f64 / union as f64
}

/// Extract important segments from long text
fn extract_important_segments(text: &str, max_length: usize) -> String {
    // Split into sentences (simple approach)
    let sentences: Vec<&str> = text
        .split(&['.', '!', '?'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if sentences.is_empty() {
        return text[..max_length.min(text.len())].to_string();
    }

    // Score sentences by importance
    let mut scored_sentences: Vec<(usize, f64, &str)> = sentences
        .iter()
        .enumerate()
        .map(|(i, &sent)| {
            let score = score_sentence_importance(sent, i, sentences.len());
            (i, score, sent)
        })
        .collect();

    // Sort by score descending
    scored_sentences.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Take top sentences up to max_length
    let mut result = Vec::new();
    let mut current_length = 0;
    let mut selected_indices = Vec::new();

    for (idx, _score, sent) in scored_sentences.iter() {
        if current_length + sent.len() <= max_length {
            selected_indices.push(*idx);
            current_length += sent.len() + 2; // +2 for ". "
        }
    }

    // Sort selected indices to maintain original order
    selected_indices.sort_unstable();

    for idx in selected_indices {
        result.push(sentences[idx]);
    }

    if result.is_empty() {
        // Fallback: just take beginning
        text[..max_length.min(text.len())].to_string()
    } else {
        result.join(". ") + "."
    }
}

/// Score sentence importance for extractive summarization
fn score_sentence_importance(sentence: &str, position: usize, total: usize) -> f64 {
    let mut score: f64 = 0.0;

    // Position weight (first and last sentences are more important)
    if position == 0 || position == total - 1 {
        score += 0.3;
    } else if position < total / 3 {
        score += 0.2;
    }

    // Length weight (very short sentences are less informative)
    let words = sentence.split_whitespace().count();
    if words >= 5 && words <= 30 {
        score += 0.2;
    } else if words > 30 {
        score += 0.1;
    }

    // Keyword weight
    let important_words = [
        "implement",
        "fix",
        "error",
        "issue",
        "resolve",
        "add",
        "update",
        "change",
        "create",
        "delete",
        "modify",
        "complete",
        "fail",
    ];
    let sent_lower = sentence.to_lowercase();
    for word in important_words {
        if sent_lower.contains(word) {
            score += 0.1;
        }
    }

    score.min(1.0)
}

/// Remove low-value turns when total size is still too large
fn remove_low_value_turns(
    turns: Vec<ConversationTurn>,
    importance_scores: &[f64],
) -> Vec<ConversationTurn> {
    let mut indexed_turns: Vec<(usize, &ConversationTurn, f64)> = turns
        .iter()
        .enumerate()
        .map(|(i, turn)| {
            let score = if i < importance_scores.len() {
                importance_scores[i]
            } else {
                0.5 // Default score for turns without importance data
            };
            (i, turn, score)
        })
        .collect();

    // Sort by importance (keep high importance turns)
    indexed_turns.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    // Keep top 80% by importance
    let keep_count = (indexed_turns.len() as f64 * 0.8).ceil() as usize;
    let mut kept_turns: Vec<(usize, ConversationTurn)> = indexed_turns
        .iter()
        .take(keep_count)
        .map(|(idx, turn, _)| (*idx, (*turn).clone()))
        .collect();

    // Sort back to original order
    kept_turns.sort_by_key(|(idx, _)| *idx);

    kept_turns.into_iter().map(|(_, turn)| turn).collect()
}

#[allow(dead_code)]
fn needs_llm_compression(turns: &[ConversationTurn]) -> bool {
    // If the compressed turns are still too large, use LLM
    let total_size: usize = turns.iter().map(|t| t.content.len() + t.role.len()).sum();

    total_size > 10_000 // 10KB threshold for LLM compression
}

#[allow(dead_code)]
#[allow(dead_code)]
async fn compress_with_llm(turns: Vec<ConversationTurn>) -> Result<Vec<ConversationTurn>, String> {
    // LLM-based compression is not yet implemented
    // For now, return the turns as-is since rule-based compression should handle most cases
    // TODO: Implement LLM-based compression using provider factory when needed
    // This would involve:
    // 1. Loading provider from factory
    // 2. Creating a summarization prompt
    // 3. Streaming the response
    // 4. Packaging result as compressed turns
    Ok(turns)
}

#[allow(dead_code)]
async fn generate_final_summary(
    turns: &[ConversationTurn],
    decisions: &[DecisionInfo],
    errors: &[ErrorInfo],
    session_start: u64,
) -> Result<ConversationSummary, String> {
    use vtcode_core::core::conversation_summarizer::{
        DecisionType, ErrorPattern, KeyDecision, TaskSummary,
    };

    // Extract completed tasks from turns
    let completed_tasks: Vec<TaskSummary> = turns
        .iter()
        .filter_map(|turn| {
            turn.task_info.as_ref().and_then(|task| {
                if task.completed && task.success {
                    Some(TaskSummary {
                        task_type: task.task_type.clone(),
                        description: task.description.clone(),
                        success: task.success,
                        duration_seconds: task.duration_seconds,
                        tools_used: task.tools_used.clone(),
                    })
                } else {
                    None
                }
            })
        })
        .collect();

    // Extract key decisions - all decisions are important
    let key_decisions: Vec<KeyDecision> = decisions
        .iter()
        .map(|d| KeyDecision {
            turn_number: d.turn_number,
            decision_type: DecisionType::ToolExecution, // Map to closest type
            description: d.description.clone(),
            rationale: d.reasoning.clone(),
            outcome: d.outcome.clone(),
            importance_score: 0.8, // Default score since DecisionInfo doesn't have confidence
        })
        .collect();

    // Identify error patterns with advanced clustering and root cause analysis
    let error_patterns: Vec<ErrorPattern> = {
        if errors.is_empty() {
            vec![]
        } else {
            analyze_error_patterns(errors)
        }
    };

    // Generate context recommendations based on patterns
    let context_recommendations: Vec<String> = {
        let mut recommendations = Vec::new();

        // Recommend based on error frequency
        if errors.len() > 5 {
            recommendations.push("Consider reviewing error handling strategies".to_string());
        }

        // Recommend based on task completion
        let total_tasks = turns.iter().filter(|t| t.task_info.is_some()).count();
        let completion_rate = if total_tasks > 0 {
            completed_tasks.len() as f64 / total_tasks as f64
        } else {
            1.0
        };

        if completion_rate < 0.5 {
            recommendations.push("Low task completion rate - review blocking issues".to_string());
        }

        // Recommend based on decision count
        if key_decisions.is_empty() && turns.len() > 10 {
            recommendations.push(
                "No significant decisions recorded - consider documenting key choices".to_string(),
            );
        }

        recommendations
    };

    // Build a comprehensive summary text
    let summary_text = {
        let mut parts = Vec::new();

        // Overview
        parts.push(format!(
            "Session summary: {} conversation turns over {} seconds.",
            turns.len(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH + Duration::from_secs(session_start))
                .map_err(|e| e.to_string())?
                .as_secs()
        ));

        // Completed tasks
        if !completed_tasks.is_empty() {
            parts.push(format!(
                "Completed {} task(s): {}",
                completed_tasks.len(),
                completed_tasks
                    .iter()
                    .map(|t| format!("{}: {}", t.task_type, t.description))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }

        // Key decisions
        if !key_decisions.is_empty() {
            parts.push(format!(
                "Key decisions: {}",
                key_decisions
                    .iter()
                    .map(|d| d.description.clone())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }

        // Errors encountered
        if !error_patterns.is_empty() {
            parts.push(format!(
                "Errors encountered: {}",
                error_patterns
                    .iter()
                    .map(|e| format!(
                        "{} ({} occurrence{})",
                        e.error_type,
                        e.frequency,
                        if e.frequency > 1 { "s" } else { "" }
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }

        // Recommendations
        if !context_recommendations.is_empty() {
            parts.push(format!(
                "Recommendations: {}",
                context_recommendations.join("; ")
            ));
        }

        parts.join(" ")
    };

    // Calculate compression ratio
    let original_size: usize = turns.iter().map(|t| t.content.len()).sum();
    let compressed_size = summary_text.len();
    let compression_ratio = if original_size > 0 {
        compressed_size as f64 / original_size as f64
    } else {
        1.0
    };

    // Calculate confidence score based on available data
    let confidence_score = {
        let mut score: f64 = 0.5; // Base score

        // Increase confidence with more data
        if turns.len() > 10 {
            score += 0.1;
        }
        if !key_decisions.is_empty() {
            score += 0.2;
        }
        if !completed_tasks.is_empty() {
            score += 0.2;
        }

        score.min(1.0f64) // Cap at 1.0
    };

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();

    let session_duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH + Duration::from_secs(session_start))
        .map_err(|e| e.to_string())?
        .as_secs();

    Ok(ConversationSummary {
        id: Uuid::new_v4().to_string(),
        timestamp: current_time,
        session_duration_seconds: session_duration,
        total_turns: turns.len(),
        key_decisions,
        completed_tasks,
        error_patterns,
        context_recommendations,
        summary_text,
        compression_ratio,
        confidence_score,
    })
}

/// Analyze error patterns with clustering and root cause identification
fn analyze_error_patterns(
    errors: &[ErrorInfo],
) -> Vec<vtcode_core::core::conversation_summarizer::ErrorPattern> {
    use vtcode_core::core::conversation_summarizer::ErrorPattern;

    // Group errors by type and analyze patterns
    let mut type_groups: std::collections::HashMap<String, Vec<&ErrorInfo>> =
        std::collections::HashMap::new();

    for error in errors {
        type_groups
            .entry(error.error_type.clone())
            .or_insert_with(Vec::new)
            .push(error);
    }

    let mut patterns = Vec::new();

    for (error_type, error_group) in type_groups {
        let frequency = error_group.len();

        // Collect unique messages
        let mut unique_messages = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for err in &error_group {
            if seen.insert(&err.message) {
                unique_messages.push(err.message.clone());
            }
        }

        // Analyze temporal clustering (are errors clustered in time?)
        let clustered = is_temporally_clustered(&error_group);

        // Analyze recoverability
        let recoverable_count = error_group.iter().filter(|e| e.recoverable).count();
        let recovery_rate = recoverable_count as f64 / frequency as f64;

        // Build comprehensive description
        let description = if frequency == 1 {
            unique_messages.join("; ")
        } else if unique_messages.len() == 1 {
            format!(
                "{} (repeated {} times{})",
                unique_messages[0],
                frequency,
                if clustered { ", clustered" } else { "" }
            )
        } else {
            format!(
                "{} distinct error messages across {} occurrences{}",
                unique_messages.len(),
                frequency,
                if clustered {
                    " (temporally clustered)"
                } else {
                    ""
                }
            )
        };

        // Generate intelligent recommendation
        let recommended_solution = generate_advanced_error_solution(
            &error_type,
            frequency,
            recovery_rate,
            clustered,
            &unique_messages,
        );

        patterns.push(ErrorPattern {
            error_type,
            frequency,
            description,
            recommended_solution,
        });
    }

    // Sort by frequency (most common first)
    patterns.sort_by(|a, b| b.frequency.cmp(&a.frequency));

    patterns
}

/// Check if errors are temporally clustered (many errors in short time)
fn is_temporally_clustered(errors: &[&ErrorInfo]) -> bool {
    if errors.len() < 3 {
        return false;
    }

    // Check if errors have turn numbers within close range
    let mut turn_numbers: Vec<usize> = errors.iter().map(|e| e.turn_number).collect();
    turn_numbers.sort_unstable();

    // Check for clusters (3+ errors within 5 turns)
    for window in turn_numbers.windows(3) {
        if window[2] - window[0] <= 5 {
            return true;
        }
    }

    false
}

/// Generate advanced error solutions based on comprehensive analysis
fn generate_advanced_error_solution(
    error_type: &str,
    frequency: usize,
    recovery_rate: f64,
    clustered: bool,
    messages: &[String],
) -> String {
    let error_lower = error_type.to_lowercase();

    // Analyze error patterns for root cause
    let mut solutions = Vec::new();

    // Frequency-based recommendations
    if frequency > 5 {
        solutions.push("High frequency error detected".to_string());
        if clustered {
            solutions.push(
                "temporal clustering suggests transient system issue or resource contention"
                    .to_string(),
            );
        } else {
            solutions.push(
                "distributed occurrence suggests systematic problem requiring code fix".to_string(),
            );
        }
    }

    // Recovery-based recommendations
    if recovery_rate < 0.3 {
        solutions.push(format!(
            "Low recovery rate ({:.0}%) indicates critical errors requiring immediate attention",
            recovery_rate * 100.0
        ));
    } else if recovery_rate > 0.7 {
        solutions.push("Good recovery rate suggests automatic retry logic is working".to_string());
    }

    // Type-specific recommendations
    if error_lower.contains("permission") || error_lower.contains("access") {
        solutions.push("Verify file/resource permissions and user access rights".to_string());
        solutions.push("Check for correct service account configuration".to_string());
    } else if error_lower.contains("network") || error_lower.contains("connection") {
        solutions.push("Check network connectivity and DNS resolution".to_string());
        if frequency > 3 {
            solutions.push("Consider implementing exponential backoff retry strategy".to_string());
        }
        if clustered {
            solutions.push(
                "Investigate network infrastructure or upstream service stability".to_string(),
            );
        }
    } else if error_lower.contains("timeout") {
        solutions.push(if frequency > 3 {
            "Recurring timeouts suggest need to increase timeout values or optimize operation performance".to_string()
        } else {
            "Review timeout configuration and operation complexity".to_string()
        });
    } else if error_lower.contains("parse")
        || error_lower.contains("syntax")
        || error_lower.contains("format")
    {
        solutions.push("Implement input validation and schema verification".to_string());
        solutions.push("Add detailed error logging to identify malformed data sources".to_string());
    } else if error_lower.contains("not found") || error_lower.contains("missing") {
        solutions.push("Verify required files/resources exist at expected paths".to_string());
        solutions.push("Check for race conditions in resource creation/access".to_string());
    } else if error_lower.contains("memory") || error_lower.contains("oom") {
        solutions.push("Analyze memory usage patterns and implement resource limits".to_string());
        solutions.push(
            "Consider implementing data streaming or chunking for large datasets".to_string(),
        );
    } else if error_lower.contains("database") || error_lower.contains("sql") {
        solutions.push("Review database query performance and indexing strategy".to_string());
        solutions.push("Check connection pool configuration and limits".to_string());
    }

    // Message pattern analysis
    if messages.len() > 3 {
        solutions.push(format!(
            "Multiple distinct error messages ({}) suggest complex root cause - recommend detailed investigation",
            messages.len()
        ));
    }

    // Generic fallback
    if solutions.is_empty() {
        if frequency > 3 {
            solutions.push("Implement comprehensive error logging and monitoring".to_string());
            solutions.push("Add retry logic with exponential backoff".to_string());
        } else {
            solutions
                .push("Review error context and implement appropriate error handling".to_string());
        }
    }

    solutions.join(". ")
}
