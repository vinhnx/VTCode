use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::scheduler::{DueSessionPrompt, ScheduleSpec, ScheduledTaskSummary};

use super::ToolRegistry;

impl ToolRegistry {
    pub async fn create_session_prompt_task(
        &self,
        name: Option<String>,
        prompt: String,
        schedule: ScheduleSpec,
        created_at: DateTime<Utc>,
    ) -> Result<ScheduledTaskSummary> {
        let mut scheduler = self.session_scheduler.lock().await;
        scheduler.create_prompt_task(name, prompt, schedule, created_at)
    }

    pub async fn list_session_tasks(&self) -> Vec<ScheduledTaskSummary> {
        let scheduler = self.session_scheduler.lock().await;
        scheduler.list()
    }

    pub async fn delete_session_task(&self, query: &str) -> Option<ScheduledTaskSummary> {
        let mut scheduler = self.session_scheduler.lock().await;
        scheduler.delete(query)
    }

    pub async fn collect_due_session_prompts(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<DueSessionPrompt>> {
        let mut scheduler = self.session_scheduler.lock().await;
        scheduler.collect_due_prompts(now)
    }
}
