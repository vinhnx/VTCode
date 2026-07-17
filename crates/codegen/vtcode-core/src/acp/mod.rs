/// ACP (Agent Collaboration Protocol) support for IDE integrations
///
/// Provides session-scoped permission management and IDE-specific helpers.
pub mod permission_cache;

pub use permission_cache::{
    AcpPermissionCache, PermissionCacheStats, PermissionGrant, ToolPermissionCache,
    ToolPermissionCacheStats,
};
