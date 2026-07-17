use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSessionAction {
    Run,
    Write,
    Poll,
    Continue,
    Inspect,
    List,
    Close,
    Code,
}
