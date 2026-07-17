use serde::Deserialize;
use serde::Serialize;
use vtcode_macros::StringNewtype;

/// Logical identifier for a VTCode exec session.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, StringNewtype,
)]
#[serde(transparent)]
pub struct ExecSessionId(String);
