pub mod in_process;
pub mod storage;
pub mod types;

pub use in_process::{InProcessTeamRunner, TeammateSpawnConfig};
pub use storage::{TeamStorage, TeamStoragePaths};
pub use types::{
    TeamConfig, TeamContext, TeamMailboxMessage, TeamProtocolMessage, TeamProtocolType, TeamRole,
    TeamTask, TeamTaskList, TeamTaskStatus, TeammateConfig,
};
