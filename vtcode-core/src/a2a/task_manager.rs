//! A2A Task Manager
//!
//! Manages task lifecycle, storage, and queries for the A2A protocol.
//! Provides an in-memory store with support for concurrent access.

use hashbrown::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::errors::{A2aError, A2aResult};
use super::rpc::{ListTasksParams, ListTasksResult, TaskPushNotificationConfig};
use super::types::{Artifact, Message, Task, TaskState, TaskStatus};

/// A2A Task Manager - handles task creation, updates, and queries
#[derive(Debug, Clone)]
pub struct TaskManager {
    /// All mutable task manager state lives behind one lock so related indexes stay in sync.
    state: Arc<RwLock<TaskManagerState>>,
    /// Maximum tasks to retain (for memory management)
    max_tasks: usize,
}

#[derive(Debug, Default)]
struct TaskManagerState {
    tasks: HashMap<String, Task>,
    contexts: HashMap<String, Vec<String>>,
    webhook_configs: HashMap<String, TaskPushNotificationConfig>,
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
            state: Arc::new(RwLock::new(TaskManagerState::default())),
            max_tasks: 1000,
        }
    }

    /// Create a new task manager with custom capacity
    pub fn with_capacity(max_tasks: usize) -> Self {
        Self {
            state: Arc::new(RwLock::new(TaskManagerState {
                tasks: HashMap::with_capacity(max_tasks.min(100)),
                contexts: HashMap::new(),
                webhook_configs: HashMap::new(),
            })),
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
        let mut state = self.state.write().await;

        if state.tasks.len() >= self.max_tasks {
            self.evict_oldest_tasks(&mut state);
        }

        state.tasks.insert(task_id.clone(), task.clone());
        if let Some(ctx_id) = context_id {
            state.contexts.entry(ctx_id).or_default().push(task_id);
        }

        task
    }

    /// Evict oldest completed tasks when at capacity
    fn evict_oldest_tasks(&self, state: &mut TaskManagerState) {
        let mut completed_tasks: Vec<_> = state
            .tasks
            .iter()
            .filter(|(_, task)| task.is_terminal())
            .map(|(id, task)| (id.clone(), task.status.timestamp))
            .collect();

        completed_tasks.sort_by(|a, b| a.1.cmp(&b.1));

        let evict_count = (self.max_tasks / 10).max(1);
        let evicted_ids: HashSet<_> = completed_tasks
            .into_iter()
            .take(evict_count)
            .map(|(id, _)| id)
            .collect();

        if evicted_ids.is_empty() {
            return;
        }

        for id in &evicted_ids {
            state.tasks.remove(id);
            state.webhook_configs.remove(id);
        }

        state.contexts.retain(|_, task_ids| {
            task_ids.retain(|task_id| !evicted_ids.contains(task_id));
            !task_ids.is_empty()
        });
    }

    /// Get a task by ID
    pub async fn get_task(&self, task_id: &str) -> Option<Task> {
        let state = self.state.read().await;
        state.tasks.get(task_id).cloned()
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
        let mut manager_state = self.state.write().await;
        let task = manager_state
            .tasks
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
        let mut state = self.state.write().await;
        let task = state
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| A2aError::TaskNotFound(task_id.to_string()))?;

        task.artifacts.push(artifact);
        Ok(task.clone())
    }

    /// Add a message to task history
    pub async fn add_message(&self, task_id: &str, message: Message) -> A2aResult<Task> {
        let mut state = self.state.write().await;
        let task = state
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| A2aError::TaskNotFound(task_id.to_string()))?;

        task.history.push(message);
        Ok(task.clone())
    }

    /// Cancel a task
    pub async fn cancel_task(&self, task_id: &str) -> A2aResult<Task> {
        let mut state = self.state.write().await;
        let task = state
            .tasks
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

    fn matches_list_filters(
        task: &Task,
        status: Option<&TaskState>,
        updated_after: Option<&chrono::DateTime<chrono::Utc>>,
    ) -> bool {
        if let Some(status) = status
            && &task.status.state != status
        {
            return false;
        }

        if let Some(updated_after) = updated_after
            && task.status.timestamp < *updated_after
        {
            return false;
        }

        true
    }

    fn clone_task_for_listing(
        task: &Task,
        include_artifacts: bool,
        history_length: Option<usize>,
    ) -> Task {
        let mut task = task.clone();

        if !include_artifacts {
            task.artifacts.clear();
        }

        if let Some(history_length) = history_length
            && task.history.len() > history_length
        {
            let trim_count = task.history.len() - history_length;
            task.history.drain(..trim_count);
        }

        task
    }

    /// List tasks with optional filtering
    pub async fn list_tasks(&self, params: ListTasksParams) -> ListTasksResult {
        let updated_after = params
            .last_updated_after
            .as_deref()
            .and_then(|after| chrono::DateTime::parse_from_rfc3339(after).ok())
            .map(|after| after.to_utc());

        let mut matching_tasks: Vec<(String, chrono::DateTime<chrono::Utc>)> = {
            let state = self.state.read().await;
            if let Some(context_id) = params.context_id.as_deref() {
                state
                    .contexts
                    .get(context_id)
                    .into_iter()
                    .flat_map(|task_ids| task_ids.iter())
                    .filter_map(|task_id| {
                        let task = state.tasks.get(task_id)?;
                        Self::matches_list_filters(
                            task,
                            params.status.as_ref(),
                            updated_after.as_ref(),
                        )
                        .then(|| (task_id.clone(), task.status.timestamp))
                    })
                    .collect()
            } else {
                state
                    .tasks
                    .iter()
                    .filter(|(_, task)| {
                        Self::matches_list_filters(
                            task,
                            params.status.as_ref(),
                            updated_after.as_ref(),
                        )
                    })
                    .map(|(task_id, task)| (task_id.clone(), task.status.timestamp))
                    .collect()
            }
        };

        matching_tasks.sort_by(|a, b| b.1.cmp(&a.1));

        let total_size = matching_tasks.len() as u32;
        let page_size = params.page_size.unwrap_or(50).min(100);
        let start_idx = params
            .page_token
            .as_ref()
            .and_then(|token| token.parse::<usize>().ok())
            .unwrap_or(0);

        let end_idx = (start_idx + page_size as usize).min(matching_tasks.len());
        let next_page_token = if end_idx < matching_tasks.len() {
            Some(end_idx.to_string())
        } else {
            None
        };

        let include_artifacts = params.include_artifacts == Some(true);
        let history_length = params.history_length.map(|len| len as usize);
        let page_task_ids: Vec<_> = matching_tasks
            .into_iter()
            .skip(start_idx)
            .take(page_size as usize)
            .collect();
        let result = if page_task_ids.is_empty() {
            Vec::new()
        } else {
            let state = self.state.read().await;
            page_task_ids
                .into_iter()
                .filter_map(|(task_id, _)| {
                    state.tasks.get(&task_id).map(|task| {
                        Self::clone_task_for_listing(task, include_artifacts, history_length)
                    })
                })
                .collect()
        };

        ListTasksResult {
            tasks: result,
            total_size: Some(total_size),
            page_size: Some(page_size),
            next_page_token,
        }
    }

    /// Get tasks by context ID
    pub async fn get_tasks_by_context(&self, context_id: &str) -> Vec<Task> {
        let state = self.state.read().await;
        state
            .contexts
            .get(context_id)
            .map(|task_ids| {
                task_ids
                    .iter()
                    .filter_map(|id| state.tasks.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get the number of tasks
    pub async fn task_count(&self) -> usize {
        self.state.read().await.tasks.len()
    }

    /// Clear all tasks (for testing)
    pub async fn clear(&self) {
        let mut state = self.state.write().await;
        state.tasks.clear();
        state.contexts.clear();
        state.webhook_configs.clear();
    }

    /// Set webhook configuration for a task
    pub async fn set_webhook_config(&self, config: TaskPushNotificationConfig) -> A2aResult<()> {
        if !config.url.starts_with("https://") && !config.url.starts_with("http://localhost") {
            return Err(A2aError::UnsupportedOperation(
                "Webhook URL must use HTTPS or be localhost".to_string(),
            ));
        }

        let mut state = self.state.write().await;
        if !state.tasks.contains_key(&config.task_id) {
            return Err(A2aError::TaskNotFound(config.task_id));
        }

        state.webhook_configs.insert(config.task_id.clone(), config);
        Ok(())
    }

    /// Get webhook configuration for a task
    pub async fn get_webhook_config(&self, task_id: &str) -> Option<TaskPushNotificationConfig> {
        let state = self.state.read().await;
        state.webhook_configs.get(task_id).cloned()
    }

    /// Remove webhook configuration for a task
    pub async fn remove_webhook_config(&self, task_id: &str) {
        let mut state = self.state.write().await;
        state.webhook_configs.remove(task_id);
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
    async fn test_eviction_cleans_context_and_webhook_indexes() {
        let manager = TaskManager::with_capacity(1);
        let task = manager.create_task(Some("ctx-1".to_string())).await;

        manager
            .update_status(&task.id, TaskState::Completed, None)
            .await
            .expect("complete");
        manager
            .set_webhook_config(TaskPushNotificationConfig {
                task_id: task.id.clone(),
                url: "https://example.com/webhook".to_string(),
                authentication: None,
            })
            .await
            .expect("set webhook");

        let replacement = manager.create_task(None).await;

        assert_eq!(manager.task_count().await, 1);
        assert!(manager.get_task(&task.id).await.is_none());
        assert!(manager.get_webhook_config(&task.id).await.is_none());
        assert!(manager.get_tasks_by_context("ctx-1").await.is_empty());
        assert_eq!(
            manager.get_task(&replacement.id).await.unwrap().id,
            replacement.id
        );
    }

    #[tokio::test]
    async fn test_list_tasks() {
        let manager = TaskManager::new();

        let _task1 = manager.create_task(Some("ctx-1".to_string())).await;
        let _task2 = manager.create_task(Some("ctx-1".to_string())).await;
        let _task3 = manager.create_task(Some("ctx-2".to_string())).await;

        let all = manager.list_tasks(ListTasksParams::default()).await;
        assert_eq!(all.tasks.len(), 3);

        let ctx1_tasks = manager
            .list_tasks(ListTasksParams {
                context_id: Some("ctx-1".to_string()),
                ..Default::default()
            })
            .await;
        assert_eq!(ctx1_tasks.tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_list_tasks_paginates_and_trims_after_sorting() {
        let manager = TaskManager::new();

        let older = manager.create_task(Some("ctx-1".to_string())).await;
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let newer = manager.create_task(Some("ctx-1".to_string())).await;

        manager
            .add_artifact(&newer.id, Artifact::text("art-1", "Generated content"))
            .await
            .expect("add artifact");
        manager
            .add_message(&newer.id, Message::user_text("Hello"))
            .await
            .expect("add msg1");
        manager
            .add_message(&newer.id, Message::agent_text("Hi there"))
            .await
            .expect("add msg2");

        let first_page = manager
            .list_tasks(ListTasksParams {
                context_id: Some("ctx-1".to_string()),
                page_size: Some(1),
                history_length: Some(1),
                include_artifacts: Some(false),
                ..Default::default()
            })
            .await;

        assert_eq!(first_page.total_size, Some(2));
        assert_eq!(first_page.next_page_token.as_deref(), Some("1"));
        assert_eq!(first_page.tasks.len(), 1);
        assert_eq!(first_page.tasks[0].id, newer.id);
        assert!(first_page.tasks[0].artifacts.is_empty());
        assert_eq!(first_page.tasks[0].history.len(), 1);
        assert_eq!(first_page.tasks[0].history[0].role, MessageRole::Agent);

        let second_page = manager
            .list_tasks(ListTasksParams {
                context_id: Some("ctx-1".to_string()),
                page_size: Some(1),
                page_token: Some("1".to_string()),
                ..Default::default()
            })
            .await;

        assert_eq!(second_page.tasks.len(), 1);
        assert_eq!(second_page.tasks[0].id, older.id);
        assert!(second_page.next_page_token.is_none());
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
