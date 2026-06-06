use serde::Deserialize;
use serde::Serialize;
use vtcode_commons::string_newtype;

string_newtype! {
    /// Logical identifier for a VTCode exec session.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct ExecSessionId
}
