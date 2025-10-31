use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::error;
use uuid::Uuid;
use vtcode_core::core::conversation_summarizer::{
    ConversationSummary, ConversationTurn, DecisionInfo, ErrorInfo,
};

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
    last_summary_size: Arc<AtomicU64>,
    last_summary_time: Arc<Mutex<Instant>>,
    min_summary_interval: Duration,
    max_concurrent_tasks: usize,
}

#[allow(dead_code)]
impl SmartSummarizer {
    /// Create a new SmartSummarizer with default settings
    pub fn new() -> Self {
        Self::with_options(Duration::from_secs(30), 4) // Default: 30s min interval, 4 max concurrent
    }

    /// Create a new SmartSummarizer with custom options
    pub fn with_options(min_summary_interval: Duration, max_concurrent_tasks: usize) -> Self {
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
            last_summary_size,
            last_summary_time,
            min_summary_interval,
            max_concurrent_tasks,
        }
    }

    /// Check if we should trigger summarization based on token usage
    pub async fn should_trigger_summarization(
        &self,
        current_tokens: usize,
        max_tokens: usize,
        min_turns_since_last: usize,
        turns_since_last: usize,
    ) -> bool {
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
    // TODO: Implement rule-based compression
    // - Remove redundant system messages
    // - Truncate long messages
    // - Merge similar consecutive messages
    turns.to_vec()
}

#[allow(dead_code)]
fn needs_llm_compression(turns: &[ConversationTurn]) -> bool {
    // If the compressed turns are still too large, use LLM
    let total_size: usize = turns.iter().map(|t| t.content.len() + t.role.len()).sum();

    total_size > 10_000 // 10KB threshold for LLM compression
}

#[allow(dead_code)]
async fn compress_with_llm(turns: Vec<ConversationTurn>) -> Result<Vec<ConversationTurn>, String> {
    // TODO: Implement LLM-based compression
    // - Use a smaller, faster model for summarization
    // - Preserve important context and decisions
    // - Return compressed turns
    Ok(turns)
}

#[allow(dead_code)]
async fn generate_final_summary(
    turns: &[ConversationTurn],
    _decisions: &[DecisionInfo],
    _errors: &[ErrorInfo],
    session_start: u64,
) -> Result<ConversationSummary, String> {
    // TODO: Implement final summary generation
    // - Extract key decisions
    // - Identify error patterns
    // - Generate concise summary
    Ok(ConversationSummary {
        id: Uuid::new_v4().to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs(),
        session_duration_seconds: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH + std::time::Duration::from_secs(session_start))
            .map_err(|e| e.to_string())?
            .as_secs(),
        total_turns: turns.len(),
        key_decisions: vec![],
        completed_tasks: vec![],
        error_patterns: vec![],
        context_recommendations: vec![],
        summary_text: "Summary not implemented yet".to_string(),
        compression_ratio: 1.0,
        confidence_score: 0.0,
    })
}
