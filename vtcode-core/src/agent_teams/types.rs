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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMailboxMessage {
    pub sender: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<u64>,
}
