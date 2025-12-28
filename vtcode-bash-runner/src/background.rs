use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, oneshot};

use crate::executor::{CommandExecutor, CommandInvocation, CommandOutput};

#[derive(Debug, Clone)]
pub struct BackgroundTaskHandle {
    pub id: String,
    pub command: String,
    pub status: BackgroundTaskStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackgroundTaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug)]
pub struct BackgroundTask {
    pub id: String,
    pub invocation: CommandInvocation,
    pub status: BackgroundTaskStatus,
    pub result: Option<Result<CommandOutput, String>>,
    pub cancel_tx: Option<oneshot::Sender<()>>,
}

pub struct BackgroundCommandManager<E: CommandExecutor> {
    executor: Arc<E>,
    tasks: Arc<RwLock<HashMap<String, BackgroundTask>>>,
    next_id: Arc<RwLock<u64>>,
}

impl<E: CommandExecutor + 'static> BackgroundCommandManager<E> {
    pub fn new(executor: E) -> Self {
        Self {
            executor: Arc::new(executor),
            tasks: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(RwLock::new(1)),
        }
    }

    pub async fn run_command(&self, invocation: CommandInvocation) -> Result<String> {
        let task_id = self.generate_task_id().await;

        let (cancel_tx, cancel_rx) = oneshot::channel();

        let task = BackgroundTask {
            id: task_id.clone(),
            invocation: invocation.clone(),
            status: BackgroundTaskStatus::Pending,
            result: None,
            cancel_tx: Some(cancel_tx),
        };

        {
            let mut tasks = self.tasks.write().await;
            tasks.insert(task_id.clone(), task);
        }

        // Update status to running
        self.update_task_status(&task_id, BackgroundTaskStatus::Running)
            .await;

        // Spawn the background task
        let executor = self.executor.clone();
        let tasks = self.tasks.clone();
        let id = task_id.clone();

        tokio::spawn(async move {
            let result = tokio::select! {
                command_result = execute_command(executor.as_ref(), &invocation) => {
                    command_result
                }
                _ = cancel_rx => {
                    // Task was cancelled
                    Err("Command was cancelled".into())
                }
            };

            let mut tasks = tasks.write().await;
            if let Some(task) = tasks.get_mut(&id) {
                task.status = match result.is_ok() {
                    true => BackgroundTaskStatus::Completed,
                    false => BackgroundTaskStatus::Failed,
                };
                task.result = Some(result.map_err(|e| e.to_string()));
                task.cancel_tx = None; // Clear the cancel sender
            }
        });

        Ok(task_id)
    }

    async fn execute_command(
        executor: &E,
        invocation: &CommandInvocation,
    ) -> Result<CommandOutput> {
        executor.execute(invocation)
    }

    pub async fn get_task(&self, task_id: &str) -> Option<BackgroundTaskHandle> {
        let tasks = self.tasks.read().await;
        tasks.get(task_id).map(|task| BackgroundTaskHandle {
            id: task.id.clone(),
            command: task.invocation.command.clone(),
            status: task.status.clone(),
        })
    }

    pub async fn get_task_output(&self, task_id: &str) -> Option<Result<CommandOutput, String>> {
        let tasks = self.tasks.read().await;
        tasks.get(task_id).and_then(|task| task.result.clone())
    }

    pub async fn list_tasks(&self) -> Vec<BackgroundTaskHandle> {
        let tasks = self.tasks.read().await;
        tasks
            .values()
            .map(|task| BackgroundTaskHandle {
                id: task.id.clone(),
                command: task.invocation.command.clone(),
                status: task.status.clone(),
            })
            .collect()
    }

    pub async fn cancel_task(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if let Some(cancel_tx) = task.cancel_tx.take() {
                if cancel_tx.send(()).is_ok() {
                    task.status = BackgroundTaskStatus::Failed;
                    return Ok(());
                }
            }
        }
        anyhow::bail!("Task not found or already completed: {}", task_id);
    }

    async fn generate_task_id(&self) -> String {
        let mut next_id = self.next_id.write().await;
        let id = format!("bg-{}", *next_id);
        *next_id += 1;
        id
    }

    async fn update_task_status(&self, task_id: &str, status: BackgroundTaskStatus) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = status;
        }
    }
}

async fn execute_command<E: CommandExecutor>(
    executor: &E,
    invocation: &CommandInvocation,
) -> Result<CommandOutput> {
    executor.execute(invocation)
}
