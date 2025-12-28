//! A2A Task Manager
//!
//! Manages task lifecycle, storage, and queries for the A2A protocol.
//! Provides an in-memory store with support for concurrent access.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::errors::{A2aError, A2aResult};
use super::rpc::{ListTasksParams, ListTasksResult, TaskPushNotificationConfig};
use super::types::{Artifact, Message, Task, TaskState, TaskStatus};

/// A2A Task Manager - handles task creation, updates, and queries
#[derive(Debug, Clone)]
pub struct TaskManager {
    /// In-memory task storage
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    /// Context ID to task IDs mapping
    contexts: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Webhook configurations per task
    webhook_configs: Arc<RwLock<HashMap<String, TaskPushNotificationConfig>>>,
    /// Maximum tasks to retain (for memory management)
    max_tasks: usize,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManager {
    /// Create a new task manager
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            contexts: Arc::new(RwLock::new(HashMap::new())),
            webhook_configs: Arc::new(RwLock::new(HashMap::new())),
            max_tasks: 1000,
        }
    }

    /// Create a new task manager with custom capacity
    pub fn with_capacity(max_tasks: usize) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::with_capacity(max_tasks.min(100)))),
            contexts: Arc::new(RwLock::new(HashMap::new())),
            webhook_configs: Arc::new(RwLock::new(HashMap::new())),
            max_tasks,
        }
    }

    /// Create a new task
    pub async fn create_task(&self, context_id: Option<String>) -> Task {
        let mut task = Task::new();
        if let Some(ref ctx_id) = context_id {
            task = task.with_context_id(ctx_id);
        }

        let task_id = task.id.clone();

        // Store the task
        {
            let mut tasks = self.tasks.write().await;

            // Evict old tasks if at capacity
            if tasks.len() >= self.max_tasks {
                self.evict_oldest_tasks(&mut tasks).await;
            }

            tasks.insert(task_id.clone(), task.clone());
        }

        // Update context mapping
        if let Some(ctx_id) = context_id {
            let mut contexts = self.contexts.write().await;
            contexts
                .entry(ctx_id)
                .or_insert_with(Vec::new)
                .push(task_id);
        }

        task
    }

    /// Evict oldest completed tasks when at capacity
    async fn evict_oldest_tasks(&self, tasks: &mut HashMap<String, Task>) {
        // Find completed tasks and sort by timestamp
        let mut completed_tasks: Vec<_> = tasks
            .iter()
            .filter(|(_, t)| t.is_terminal())
            .map(|(id, t)| (id.clone(), t.status.timestamp))
            .collect();

        completed_tasks.sort_by(|a, b| a.1.cmp(&b.1));

        // Evict oldest 10% of completed tasks
        let evict_count = (self.max_tasks / 10).max(1);
        for (id, _) in completed_tasks.into_iter().take(evict_count) {
            tasks.remove(&id);
        }
    }

    /// Get a task by ID
    pub async fn get_task(&self, task_id: &str) -> Option<Task> {
        let tasks = self.tasks.read().await;
        tasks.get(task_id).cloned()
    }

    /// Get a task by ID, returning an error if not found
    pub async fn get_task_or_error(&self, task_id: &str) -> A2aResult<Task> {
        self.get_task(task_id)
            .await
            .ok_or_else(|| A2aError::TaskNotFound(task_id.to_string()))
    }

    /// Update task status
    pub async fn update_status(
        &self,
        task_id: &str,
        state: TaskState,
        message: Option<Message>,
    ) -> A2aResult<Task> {
        let mut tasks = self.tasks.write().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| A2aError::TaskNotFound(task_id.to_string()))?;

        task.status = match message {
            Some(msg) => TaskStatus::with_message(state, msg),
            None => TaskStatus::new(state),
        };

        Ok(task.clone())
    }

    /// Add an artifact to a task
    pub async fn add_artifact(&self, task_id: &str, artifact: Artifact) -> A2aResult<Task> {
        let mut tasks = self.tasks.write().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| A2aError::TaskNotFound(task_id.to_string()))?;

        task.artifacts.push(artifact);
        Ok(task.clone())
    }

    /// Add a message to task history
    pub async fn add_message(&self, task_id: &str, message: Message) -> A2aResult<Task> {
        let mut tasks = self.tasks.write().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| A2aError::TaskNotFound(task_id.to_string()))?;

        task.history.push(message);
        Ok(task.clone())
    }

    /// Cancel a task
    pub async fn cancel_task(&self, task_id: &str) -> A2aResult<Task> {
        let mut tasks = self.tasks.write().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| A2aError::TaskNotFound(task_id.to_string()))?;

        if !task.is_cancelable() {
            return Err(A2aError::TaskNotCancelable(format!(
                "Task {} is in state {:?} and cannot be canceled",
                task_id, task.status.state
            )));
        }

        task.status = TaskStatus::new(TaskState::Canceled);
        Ok(task.clone())
    }

    /// List tasks with optional filtering
    pub async fn list_tasks(&self, params: ListTasksParams) -> ListTasksResult {
        let tasks = self.tasks.read().await;

        let mut result: Vec<Task> = tasks
            .values()
            .filter(|task| {
                // Filter by context ID
                if let Some(ref ctx_id) = params.context_id {
                    if task.context_id.as_ref() != Some(ctx_id) {
                        return false;
                    }
                }

                // Filter by status
                if let Some(ref status) = params.status {
                    if &task.status.state != status {
                        return false;
                    }
                }

                // Filter by last updated
                if let Some(ref after) = params.last_updated_after {
                    if let Ok(after_time) = chrono::DateTime::parse_from_rfc3339(after) {
                        if task.status.timestamp < after_time.to_utc() {
                            return false;
                        }
                    }
                }

                true
            })
            .cloned()
            .collect();

        // Sort by timestamp (newest first)
        result.sort_by(|a, b| b.status.timestamp.cmp(&a.status.timestamp));

        // Apply pagination
        let total_size = result.len() as u32;
        let page_size = params.page_size.unwrap_or(50).min(100);
        let start_idx = params
            .page_token
            .as_ref()
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(0);

        let end_idx = (start_idx + page_size as usize).min(result.len());
        let next_page_token = if end_idx < result.len() {
            Some(end_idx.to_string())
        } else {
            None
        };

        result = result
            .into_iter()
            .skip(start_idx)
            .take(page_size as usize)
            .collect();

        // Optionally trim artifacts
        if params.include_artifacts != Some(true) {
            for task in &mut result {
                task.artifacts.clear();
            }
        }

        // Optionally trim history
        if let Some(history_len) = params.history_length {
            for task in &mut result {
                if task.history.len() > history_len as usize {
                    let start = task.history.len() - history_len as usize;
                    task.history = task.history[start..].to_vec();
                }
            }
        }

        ListTasksResult {
            tasks: result,
            total_size: Some(total_size),
            page_size: Some(page_size),
            next_page_token,
        }
    }

    /// Get tasks by context ID
    pub async fn get_tasks_by_context(&self, context_id: &str) -> Vec<Task> {
        let contexts = self.contexts.read().await;
        if let Some(task_ids) = contexts.get(context_id) {
            let tasks = self.tasks.read().await;
            task_ids
                .iter()
                .filter_map(|id| tasks.get(id).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get the number of tasks
    pub async fn task_count(&self) -> usize {
        self.tasks.read().await.len()
    }

    /// Clear all tasks (for testing)
    pub async fn clear(&self) {
        self.tasks.write().await.clear();
        self.contexts.write().await.clear();
        self.webhook_configs.write().await.clear();
    }

    /// Set webhook configuration for a task
    pub async fn set_webhook_config(&self, config: TaskPushNotificationConfig) -> A2aResult<()> {
        // Validate webhook URL (basic SSRF protection)
        if !config.url.starts_with("https://") && !config.url.starts_with("http://localhost") {
            return Err(A2aError::UnsupportedOperation(
                "Webhook URL must use HTTPS or be localhost".to_string(),
            ));
        }

        // Verify task exists
        let _ = self.get_task_or_error(&config.task_id).await?;

        let mut configs = self.webhook_configs.write().await;
        configs.insert(config.task_id.clone(), config);
        Ok(())
    }

    /// Get webhook configuration for a task
    pub async fn get_webhook_config(&self, task_id: &str) -> Option<TaskPushNotificationConfig> {
        let configs = self.webhook_configs.read().await;
        configs.get(task_id).cloned()
    }

    /// Remove webhook configuration for a task
    pub async fn remove_webhook_config(&self, task_id: &str) {
        let mut configs = self.webhook_configs.write().await;
        configs.remove(task_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a::types::MessageRole;

    #[tokio::test]
    async fn test_create_task() {
        let manager = TaskManager::new();
        let task = manager.create_task(None).await;

        assert!(!task.id.is_empty());
        assert_eq!(task.state(), TaskState::Submitted);
        assert_eq!(manager.task_count().await, 1);
    }

    #[tokio::test]
    async fn test_create_task_with_context() {
        let manager = TaskManager::new();
        let task = manager.create_task(Some("ctx-1".to_string())).await;

        assert_eq!(task.context_id, Some("ctx-1".to_string()));

        let tasks = manager.get_tasks_by_context("ctx-1").await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task.id);
    }

    #[tokio::test]
    async fn test_get_task() {
        let manager = TaskManager::new();
        let task = manager.create_task(None).await;

        let retrieved = manager.get_task(&task.id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, task.id);

        let missing = manager.get_task("nonexistent").await;
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_update_status() {
        let manager = TaskManager::new();
        let task = manager.create_task(None).await;

        let updated = manager
            .update_status(&task.id, TaskState::Working, None)
            .await
            .expect("update");
        assert_eq!(updated.state(), TaskState::Working);

        let msg = Message::agent_text("Task completed successfully");
        let completed = manager
            .update_status(&task.id, TaskState::Completed, Some(msg))
            .await
            .expect("complete");
        assert_eq!(completed.state(), TaskState::Completed);
        assert!(completed.status.message.is_some());
    }

    #[tokio::test]
    async fn test_add_artifact() {
        let manager = TaskManager::new();
        let task = manager.create_task(None).await;

        let artifact = Artifact::text("art-1", "Generated content");
        let updated = manager
            .add_artifact(&task.id, artifact)
            .await
            .expect("add artifact");
        assert_eq!(updated.artifacts.len(), 1);
        assert_eq!(updated.artifacts[0].id, "art-1");
    }

    #[tokio::test]
    async fn test_cancel_task() {
        let manager = TaskManager::new();
        let task = manager.create_task(None).await;

        let canceled = manager.cancel_task(&task.id).await.expect("cancel");
        assert_eq!(canceled.state(), TaskState::Canceled);
    }

    #[tokio::test]
    async fn test_cancel_completed_task_fails() {
        let manager = TaskManager::new();
        let task = manager.create_task(None).await;

        manager
            .update_status(&task.id, TaskState::Completed, None)
            .await
            .expect("complete");

        let result = manager.cancel_task(&task.id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_tasks() {
        let manager = TaskManager::new();

        // Create multiple tasks
        let _task1 = manager.create_task(Some("ctx-1".to_string())).await;
        let _task2 = manager.create_task(Some("ctx-1".to_string())).await;
        let _task3 = manager.create_task(Some("ctx-2".to_string())).await;

        // List all
        let all = manager.list_tasks(ListTasksParams::default()).await;
        assert_eq!(all.tasks.len(), 3);

        // List by context
        let ctx1_tasks = manager
            .list_tasks(ListTasksParams {
                context_id: Some("ctx-1".to_string()),
                ..Default::default()
            })
            .await;
        assert_eq!(ctx1_tasks.tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_add_message_to_history() {
        let manager = TaskManager::new();
        let task = manager.create_task(None).await;

        let msg1 = Message::user_text("Hello");
        let msg2 = Message::agent_text("Hi there!");

        manager.add_message(&task.id, msg1).await.expect("add msg1");
        let updated = manager.add_message(&task.id, msg2).await.expect("add msg2");

        assert_eq!(updated.history.len(), 2);
        assert_eq!(updated.history[0].role, MessageRole::User);
        assert_eq!(updated.history[1].role, MessageRole::Agent);
    }
}
