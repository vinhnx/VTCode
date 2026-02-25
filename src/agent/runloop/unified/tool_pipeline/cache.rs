use serde_json::Value;
use vtcode_core::tools::result_cache::ToolCacheKey;

/// Determine if a tool is cacheable based on tool type and arguments
/// This function implements enhanced caching logic to support more tools
pub(super) fn is_tool_cacheable(tool_name: &str, args: &Value) -> bool {
    // Always cache these read-only tools (original set)
    if matches!(
        tool_name,
        "read_file" | "list_files" | "grep_search" | "find_files"
    ) {
        return true;
    }

    // Cache search tools with stable arguments
    if matches!(tool_name, "search_tools" | "get_errors" | "agent_info") {
        // These tools typically have stable results within a session
        return true;
    }

    false
}

/// Enhanced cache key creation that includes workspace context in the target path
/// This prevents cache collisions between different workspaces
pub(super) fn create_enhanced_cache_key(
    tool_name: &str,
    args: &Value,
    cache_target: &str,
    workspace: &str,
) -> ToolCacheKey {
    // For file-based tools, include workspace in the target path to ensure uniqueness
    // For non-file tools, use a workspace-specific target path
    let enhanced_target = if cache_target.starts_with('/') || cache_target.contains(':') {
        // Absolute path or special path - keep as is
        cache_target.to_string()
    } else {
        // Relative path - prefix with workspace to ensure uniqueness
        format!("{}/{}", workspace, cache_target)
    };

    ToolCacheKey::from_json(tool_name, args, &enhanced_target)
}

pub(super) fn cache_target_path(tool_name: &str, args: &Value) -> String {
    if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
        return path.to_string();
    }
    if let Some(root) = args.get("root").and_then(|v| v.as_str()) {
        return root.to_string();
    }
    if let Some(target) = args.get("target_path").and_then(|v| v.as_str()) {
        return target.to_string();
    }
    if let Some(dir) = args.get("dir").and_then(|v| v.as_str()) {
        return dir.to_string();
    }

    tool_name.to_string()
}
