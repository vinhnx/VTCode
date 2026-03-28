use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalAgentKind {
    Delegated,
    Background,
}

impl LocalAgentKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delegated => "delegated",
            Self::Background => "background",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalAgentEntry {
    pub id: String,
    pub display_label: String,
    pub agent_name: String,
    pub color: Option<String>,
    pub kind: LocalAgentKind,
    pub status: String,
    pub summary: Option<String>,
    pub preview: String,
    pub transcript_path: Option<PathBuf>,
}
