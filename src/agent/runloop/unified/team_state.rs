use std::collections::BTreeMap;

use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamTaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct TeamTask {
    pub id: u64,
    pub description: String,
    pub status: TeamTaskStatus,
    pub assigned_to: Option<String>,
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Teammate {
    pub name: String,
    pub subagent_type: String,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TeamState {
    pub name: String,
    pub default_subagent: String,
    pub teammates: BTreeMap<String, Teammate>,
    pub tasks: Vec<TeamTask>,
    pub next_task_id: u64,
    pub busy: bool,
}

impl TeamState {
    pub fn new(name: String, default_subagent: String) -> Self {
        Self {
            name,
            default_subagent,
            teammates: BTreeMap::new(),
            tasks: Vec::new(),
            next_task_id: 1,
            busy: false,
        }
    }

    pub fn add_teammate(&mut self, name: String, subagent_type: String) -> Result<()> {
        if self.teammates.contains_key(&name) {
            bail!("Teammate '{}' already exists.", name);
        }
        self.teammates.insert(
            name.clone(),
            Teammate {
                name,
                subagent_type,
                agent_id: None,
            },
        );
        Ok(())
    }

    pub fn remove_teammate(&mut self, name: &str) -> Result<()> {
        if self.tasks.iter().any(|task| {
            task.assigned_to.as_deref() == Some(name)
                && matches!(
                    task.status,
                    TeamTaskStatus::Pending | TeamTaskStatus::InProgress
                )
        }) {
            bail!("Teammate '{}' has active or pending tasks.", name);
        }
        if self.teammates.remove(name).is_none() {
            bail!("Teammate '{}' not found.", name);
        }
        Ok(())
    }

    pub fn add_task(&mut self, description: String) -> u64 {
        let id = self.next_task_id;
        self.next_task_id = self.next_task_id.saturating_add(1);
        self.tasks.push(TeamTask {
            id,
            description,
            status: TeamTaskStatus::Pending,
            assigned_to: None,
            result_summary: None,
        });
        id
    }

    pub fn find_task_mut(&mut self, task_id: u64) -> Option<&mut TeamTask> {
        self.tasks.iter_mut().find(|task| task.id == task_id)
    }
}
