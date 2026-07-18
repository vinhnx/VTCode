//! Tool semantic taxonomy — the "what does this tool do" dimension.
//!
//! This is complementary to the surface-level `ToolKind` in `vtcode-core`
//! (Function / Mcp / Custom). These variants answer "what category of work
//! does this tool perform", which is useful for:
//! - capability negotiation ("does this session support LSP tools?")
//! - token-budget policy (search tools are cheap, goal tools are expensive)
//! - UI grouping (file tools vs shell tools vs web tools)

use serde::{Deserialize, Serialize};

/// Semantic category of a tool — what it does, not how it is called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// Read-only file inspection (read_file, view, cat, etc.)
    Read,
    /// File creation and mutation (write_file, apply_patch, edit, etc.)
    Edit,
    /// File or directory deletion
    Delete,
    /// Directory or file listing (ls, find, tree, etc.)
    ListDir,
    /// File or path movement (mv, rename, etc.)
    Move,
    /// Code or text search (grep, rg, search, etc.)
    Search,
    /// Language-server protocol operations (goto, hover, diagnostics, etc.)
    Lsp,
    /// Shell or system command execution
    Execute,
    /// Planning, tracking, or goal management
    Plan,
    /// Web search
    WebSearch,
    /// Web fetch / scrape
    WebFetch,
    /// Background or scheduled task management
    Background,
    /// Skill loading or invocation
    Skill,
    /// Memory search or retrieval
    Memory,
    /// Goal or objective management
    Goal,
    /// Catch-all for unclassified tools
    Other,
}

impl ToolKind {
    /// Short stable label used in telemetry and grouping.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Delete => "delete",
            Self::ListDir => "list_dir",
            Self::Move => "move",
            Self::Search => "search",
            Self::Lsp => "lsp",
            Self::Execute => "execute",
            Self::Plan => "plan",
            Self::WebSearch => "web_search",
            Self::WebFetch => "web_fetch",
            Self::Background => "background",
            Self::Skill => "skill",
            Self::Memory => "memory",
            Self::Goal => "goal",
            Self::Other => "other",
        }
    }
}

impl std::fmt::Display for ToolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Logical namespace for a tool — who "owns" it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolNamespace {
    /// Built-in vtcode tool
    Builtin,
    /// MCP server tool
    Mcp,
    /// Skill-provided tool
    Skill,
    /// Extension or plugin tool
    Extension,
    /// User-defined alias or wrapper
    Alias,
    /// Unknown / fallback
    Other,
}

impl ToolNamespace {
    /// Short stable label used in telemetry and grouping.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Mcp => "mcp",
            Self::Skill => "skill",
            Self::Extension => "extension",
            Self::Alias => "alias",
            Self::Other => "other",
        }
    }
}

impl std::fmt::Display for ToolNamespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Canonical metadata envelope attached to a tool registration.
///
/// The `_meta` wrapper keeps the envelope distinct from the tool's functional
/// schema so downstream consumers can strip or override it without affecting
/// parameter validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalToolMeta {
    /// Semantic category of the tool.
    pub kind: ToolKind,
    /// Logical owner of the tool.
    pub namespace: ToolNamespace,
    /// Human-readable label for UI grouping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Short description of the tool's side-effects (mutating, read-only, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_effects: Option<String>,
    /// Estimated token cost bucket (small < 500, medium < 2000, large >= 2000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_bucket: Option<TokenBucket>,
}

/// Rough token-cost bucket for a tool invocation (input + output estimate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenBucket {
    Small,
    Medium,
    Large,
}

impl CanonicalToolMeta {
    /// Create a minimal metadata entry with kind and namespace.
    pub fn new(kind: ToolKind, namespace: ToolNamespace) -> Self {
        Self {
            kind,
            namespace,
            label: None,
            side_effects: None,
            token_bucket: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_kind_round_trips() {
        for kind in [
            ToolKind::Read,
            ToolKind::Edit,
            ToolKind::Delete,
            ToolKind::ListDir,
            ToolKind::Move,
            ToolKind::Search,
            ToolKind::Lsp,
            ToolKind::Execute,
            ToolKind::Plan,
            ToolKind::WebSearch,
            ToolKind::WebFetch,
            ToolKind::Background,
            ToolKind::Skill,
            ToolKind::Memory,
            ToolKind::Goal,
            ToolKind::Other,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: ToolKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, back);
            assert!(!kind.as_str().is_empty());
        }
    }

    #[test]
    fn tool_namespace_round_trips() {
        for ns in [
            ToolNamespace::Builtin,
            ToolNamespace::Mcp,
            ToolNamespace::Skill,
            ToolNamespace::Extension,
            ToolNamespace::Alias,
            ToolNamespace::Other,
        ] {
            let json = serde_json::to_string(&ns).unwrap();
            let back: ToolNamespace = serde_json::from_str(&json).unwrap();
            assert_eq!(ns, back);
            assert!(!ns.as_str().is_empty());
        }
    }

    #[test]
    fn canonical_tool_meta_serializes_without_label() {
        let meta = CanonicalToolMeta::new(ToolKind::Search, ToolNamespace::Builtin);
        let json = serde_json::to_string(&meta).unwrap();
        assert!(!json.contains("label"));
        assert!(json.contains("search"));
        assert!(json.contains("builtin"));
    }

    #[test]
    fn canonical_tool_meta_serializes_with_optional_fields() {
        let mut meta = CanonicalToolMeta::new(ToolKind::Execute, ToolNamespace::Builtin);
        meta.label = Some("Shell".to_string());
        meta.side_effects = Some("mutating".to_string());
        meta.token_bucket = Some(TokenBucket::Medium);
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("Shell"));
        assert!(json.contains("mutating"));
        assert!(json.contains("medium"));
    }
}
