use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use chrono::Utc;

use vtcode_core::agent_teams::storage::build_task_completion_details;
use vtcode_core::agent_teams::{
    TeamConfig, TeamMailboxMessage, TeamProtocolMessage, TeamStorage, TeamTask, TeamTaskList,
    TeamTaskStatus, TeammateConfig,
};

#[derive(Debug, Clone)]
pub struct TeamState {
    pub config: TeamConfig,
    pub tasks: TeamTaskList,
    pub storage: TeamStorage,
    pub mailbox_offsets: HashMap<String, u64>,
}

impl TeamState {
    pub async fn create(
        storage: TeamStorage,
        name: String,
        default_subagent: String,
        teammates: Vec<TeammateConfig>,
    ) -> Result<Self> {
        let config = TeamConfig {
            name: name.clone(),
            created_at: Utc::now(),
            default_subagent,
            teammates,
            active_teammate: None,
            lead_session_id: None,
            version: 1,
        };

        storage.save_team_config(&config).await?;
        storage.save_tasks(&name, &TeamTaskList::default()).await?;

        Ok(Self {
            config,
            tasks: TeamTaskList::default(),
            storage,
            mailbox_offsets: HashMap::new(),
        })
    }

    pub async fn load(storage: TeamStorage, team_name: &str) -> Result<Self> {
        let Some(config) = storage.load_team_config(team_name).await? else {
            bail!("Team '{}' not found.", team_name);
        };
        let tasks = storage.load_tasks(team_name).await?;
        Ok(Self {
            config,
            tasks,
            storage,
            mailbox_offsets: HashMap::new(),
        })
    }

    pub fn teammate_names(&self) -> Vec<String> {
        self.config
            .teammates
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    pub fn active_teammate(&self) -> Option<&str> {
        self.config.active_teammate.as_deref()
    }

    pub async fn set_active_teammate(&mut self, name: Option<String>) -> Result<()> {
        self.config.active_teammate = name;
        self.storage.save_team_config(&self.config).await
    }

    pub async fn add_teammate(
        &mut self,
        name: String,
        subagent_type: String,
        model: Option<String>,
    ) -> Result<()> {
        if self
            .config
            .teammates
            .iter()
            .any(|teammate| teammate.name == name)
        {
            bail!("Teammate '{}' already exists.", name);
        }
        self.config.teammates.push(TeammateConfig {
            name,
            subagent_type,
            model,
            session_id: None,
        });
        self.storage.save_team_config(&self.config).await
    }

    pub async fn remove_teammate(&mut self, name: &str) -> Result<()> {
        let has_active = self.tasks.tasks.iter().any(|task| {
            task.assigned_to.as_deref() == Some(name)
                && matches!(
                    task.status,
                    TeamTaskStatus::Pending | TeamTaskStatus::InProgress
                )
        });
        if has_active {
            bail!("Teammate '{}' has active or pending tasks.", name);
        }

        let before = self.config.teammates.len();
        self.config.teammates.retain(|t| t.name != name);
        if self.config.teammates.len() == before {
            bail!("Teammate '{}' not found.", name);
        }
        if self.config.active_teammate.as_deref() == Some(name) {
            self.config.active_teammate = None;
        }
        self.storage.save_team_config(&self.config).await
    }

    pub async fn add_task(&mut self, description: String, depends_on: Vec<u64>) -> Result<u64> {
        let team_name = self.config.name.clone();
        let tasks = self
            .storage
            .with_task_lock(&team_name, move |tasks| {
                let id = tasks.next_task_id;
                tasks.next_task_id = tasks.next_task_id.saturating_add(1);
                let now = Utc::now();
                tasks.tasks.push(TeamTask {
                    id,
                    description,
                    status: TeamTaskStatus::Pending,
                    assigned_to: None,
                    depends_on,
                    result_summary: None,
                    created_at: now,
                    updated_at: now,
                });
                Ok((id, tasks.clone()))
            })
            .await?;

        self.tasks = tasks.1;
        Ok(tasks.0)
    }

    pub async fn assign_task(&mut self, task_id: u64, teammate: &str) -> Result<()> {
        self.update_task_assignment(task_id, Some(teammate.to_string()))
            .await
    }

    pub async fn claim_task(&mut self, task_id: u64, teammate: &str) -> Result<()> {
        self.update_task_assignment(task_id, Some(teammate.to_string()))
            .await
    }

    async fn update_task_assignment(
        &mut self,
        task_id: u64,
        teammate: Option<String>,
    ) -> Result<()> {
        let team_name = self.config.name.clone();
        let tasks = self
            .storage
            .with_task_lock(&team_name, move |tasks| {
                let task_index = tasks
                    .tasks
                    .iter()
                    .position(|task| task.id == task_id)
                    .ok_or_else(|| anyhow!("Task #{} not found.", task_id))?;

                if tasks.tasks[task_index].status != TeamTaskStatus::Pending {
                    bail!("Only pending tasks can be assigned.");
                }

                let depends_on = tasks.tasks[task_index].depends_on.clone();
                if !dependencies_met(tasks, &depends_on) {
                    bail!("Task #{} is blocked by dependencies.", task_id);
                }

                let task = &mut tasks.tasks[task_index];
                task.status = TeamTaskStatus::InProgress;
                task.assigned_to = teammate;
                task.updated_at = Utc::now();
                Ok(tasks.clone())
            })
            .await?;

        self.tasks = tasks;
        Ok(())
    }

    pub async fn complete_task(
        &mut self,
        task_id: u64,
        success: bool,
        summary: Option<String>,
    ) -> Result<(Option<String>, Option<serde_json::Value>)> {
        let team_name = self.config.name.clone();
        let tasks = self
            .storage
            .with_task_lock(&team_name, move |tasks| {
                let task = tasks
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == task_id)
                    .ok_or_else(|| anyhow!("Task #{} not found.", task_id))?;

                task.status = if success {
                    TeamTaskStatus::Completed
                } else {
                    TeamTaskStatus::Failed
                };
                task.result_summary = summary;
                task.updated_at = Utc::now();
                Ok(tasks.clone())
            })
            .await?;

        self.tasks = tasks.clone();

        let task = tasks.tasks.iter().find(|task| task.id == task_id);
        let assigned_to = task.and_then(|task| task.assigned_to.clone());
        let details = task.map(|task| {
            build_task_completion_details(
                task.id,
                task.assigned_to.as_deref(),
                task.result_summary.as_deref(),
            )
        });

        Ok((assigned_to, details))
    }

    pub async fn reload_tasks(&mut self) -> Result<()> {
        self.tasks = self.storage.load_tasks(&self.config.name).await?;
        Ok(())
    }

    pub async fn send_message(
        &self,
        recipient: &str,
        sender: &str,
        content: String,
        task_id: Option<u64>,
    ) -> Result<()> {
        let message = TeamMailboxMessage {
            sender: sender.to_string(),
            content: Some(content),
            protocol: None,
            id: None,
            read: false,
            timestamp: Utc::now(),
            task_id,
        };
        self.storage
            .append_mailbox_message(&self.config.name, recipient, &message)
            .await
    }

    pub async fn send_protocol(
        &self,
        recipient: &str,
        sender: &str,
        protocol: TeamProtocolMessage,
        task_id: Option<u64>,
    ) -> Result<()> {
        let message = TeamMailboxMessage {
            sender: sender.to_string(),
            content: None,
            protocol: Some(protocol),
            id: None,
            read: false,
            timestamp: Utc::now(),
            task_id,
        };
        self.storage
            .append_mailbox_message(&self.config.name, recipient, &message)
            .await
    }

    pub async fn load_persisted_offset(&mut self, recipient: &str) -> Result<()> {
        let offset = self
            .storage
            .load_mailbox_offset(&self.config.name, recipient)
            .await?;
        if offset > 0 {
            self.mailbox_offsets.insert(recipient.to_string(), offset);
        }
        Ok(())
    }

    pub fn prompt_snapshot(&self) -> String {
        self.config.prompt_snapshot(&self.tasks)
    }

    pub async fn read_mailbox(&mut self, recipient: &str) -> Result<Vec<TeamMailboxMessage>> {
        let offset = self.mailbox_offsets.get(recipient).copied().unwrap_or(0);
        let (messages, new_offset) = self
            .storage
            .read_mailbox_since(&self.config.name, recipient, offset)
            .await?;
        self.mailbox_offsets
            .insert(recipient.to_string(), new_offset);
        self.storage
            .save_mailbox_offset(&self.config.name, recipient, new_offset)
            .await?;
        Ok(messages)
    }
}

fn dependencies_met(tasks: &TeamTaskList, depends_on: &[u64]) -> bool {
    depends_on.iter().all(|dep| {
        tasks
            .tasks
            .iter()
            .any(|task| task.id == *dep && matches!(task.status, TeamTaskStatus::Completed))
    })
}
