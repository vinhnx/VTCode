use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedExecAction {
    Run,
    Write,
    Poll,
    Continue,
    Inspect,
    List,
    Close,
    Code,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedFileAction {
    Read,
    Write,
    Edit,
    Patch,
    Delete,
    Move,
    Copy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedSearchAction {
    Grep,
    List,
    Intelligence,
    Tools,
    Errors,
    Agent,
    Web,
    Skill,
}
