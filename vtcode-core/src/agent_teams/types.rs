use std::fmt::Write;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use vtcode_config::agent_teams::TeammateMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRole {
    Lead,
    Teammate,
}

impl TeamRole {
    pub fn as_str(self) -> &'static str {
        match self {
            TeamRole::Lead => "lead",
            TeamRole::Teammate => "teammate",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamContext {
    pub team_name: String,
    pub role: TeamRole,
    pub teammate_name: Option<String>,
    pub mode: TeammateMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamConfig {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub default_subagent: String,
    pub teammates: Vec<TeammateConfig>,
    #[serde(default)]
    pub active_teammate: Option<String>,
    #[serde(default)]
    pub lead_session_id: Option<String>,
    #[serde(default)]
    pub version: u32,
}

impl TeamConfig {
    pub fn prompt_snapshot(&self, tasks: &TeamTaskList) -> String {
        let mut out = format!("## Team: {}\n", self.name);

        // Teammates with metadata
        if self.teammates.is_empty() {
            let _ = writeln!(out, "Teammates: (none)");
        } else {
            let _ = writeln!(out, "Teammates:");
            for t in &self.teammates {
                let model = t.model.as_deref().unwrap_or("default");
                let active_marker = if self.active_teammate.as_deref() == Some(&t.name) {
                    " ← active"
                } else {
                    ""
                };
                let _ = writeln!(
                    out,
                    "  - {} ({}, {}){active_marker}",
                    t.name, t.subagent_type, model,
                );
            }
        }

        // Active tasks
        let active_tasks: Vec<&TeamTask> = tasks
            .tasks
            .iter()
            .filter(|t| matches!(t.status, TeamTaskStatus::Pending | TeamTaskStatus::InProgress))
            .collect();
        if active_tasks.is_empty() {
            let _ = writeln!(out, "Tasks: none pending");
        } else {
            let _ = writeln!(out, "Tasks ({} active):", active_tasks.len());
            for t in &active_tasks {
                let assignee = t.assigned_to.as_deref().unwrap_or("unassigned");
                let status = match t.status {
                    TeamTaskStatus::Pending => "pending",
                    TeamTaskStatus::InProgress => "in_progress",
                    _ => "other",
                };
                let desc = truncate_str(&t.description, 200);
                let _ = writeln!(out, "  - [{status}] #{}: {desc} ({assignee})", t.id);
            }
        }

        // Recent outcomes (last 3 completed/failed with summaries)
        let mut recent: Vec<&TeamTask> = tasks
            .tasks
            .iter()
            .filter(|t| {
                matches!(t.status, TeamTaskStatus::Completed | TeamTaskStatus::Failed)
                    && t.result_summary.is_some()
            })
            .collect();
        recent.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        let recent = &recent[..recent.len().min(3)];
        if !recent.is_empty() {
            let _ = writeln!(out, "Recent outcomes:");
            for t in recent {
                let status = match t.status {
                    TeamTaskStatus::Completed => "done",
                    TeamTaskStatus::Failed => "failed",
                    _ => "other",
                };
                let summary = t
                    .result_summary
                    .as_deref()
                    .map(|s| truncate_str(s, 150))
                    .unwrap_or_default();
                let _ = writeln!(out, "  - [{status}] #{}: {summary}", t.id);
            }
        }

        out
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let boundary = s
            .char_indices()
            .take_while(|(i, _)| *i < max)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(max);
        format!("{}…", &s[..boundary])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateConfig {
    pub name: String,
    pub subagent_type: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamTaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTask {
    pub id: u64,
    pub description: String,
    pub status: TeamTaskStatus,
    #[serde(default)]
    pub assigned_to: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<u64>,
    #[serde(default)]
    pub result_summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTaskList {
    pub next_task_id: u64,
    pub tasks: Vec<TeamTask>,
}

impl Default for TeamTaskList {
    fn default() -> Self {
        Self {
            next_task_id: 1,
            tasks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamProtocolType {
    IdleNotification,
    ShutdownRequest,
    ShutdownApproved,
    TaskUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamProtocolMessage {
    pub r#type: TeamProtocolType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMailboxMessage {
    pub sender: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<TeamProtocolMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default)]
    pub read: bool,
}
