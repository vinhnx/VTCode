use serde_json::Value;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::tool_intent;

pub(crate) fn read_file_path_arg(args: &Value) -> Option<&str> {
    let obj = args.as_object()?;
    for key in ["path", "file_path", "filepath", "target_path"] {
        if let Some(path) = obj.get(key).and_then(Value::as_str) {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

pub(crate) fn is_read_file_style_call(canonical_tool_name: &str, args: &Value) -> bool {
    match canonical_tool_name {
        tool_names::READ_FILE => true,
        tool_names::UNIFIED_FILE => tool_intent::unified_file_action(args)
            .unwrap_or("read")
            .eq_ignore_ascii_case("read"),
        _ => false,
    }
}

fn looks_like_tool_output_spool_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.contains(".vtcode/context/tool_outputs/")
}

pub(crate) fn spool_chunk_read_path<'a>(
    canonical_tool_name: &str,
    args: &'a Value,
) -> Option<&'a str> {
    if !is_read_file_style_call(canonical_tool_name, args) {
        return None;
    }

    let path = read_file_path_arg(args)?;
    if looks_like_tool_output_spool_path(path) {
        Some(path)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vtcode_core::config::constants::tools;

    use super::spool_chunk_read_path;

    #[test]
    fn spool_chunk_read_path_matches_read_file_spool_reads() {
        let args = json!({
            "path": ".vtcode/context/tool_outputs/unified_exec_123.txt",
            "offset": 41,
            "limit": 40
        });

        assert_eq!(
            spool_chunk_read_path(tools::READ_FILE, &args),
            Some(".vtcode/context/tool_outputs/unified_exec_123.txt")
        );
    }

    #[test]
    fn spool_chunk_read_path_matches_unified_file_read_spool_reads() {
        let args = json!({
            "action": "read",
            "path": ".vtcode/context/tool_outputs/unified_exec_456.txt",
            "offset": 81,
            "limit": 40
        });

        assert_eq!(
            spool_chunk_read_path(tools::UNIFIED_FILE, &args),
            Some(".vtcode/context/tool_outputs/unified_exec_456.txt")
        );
    }

    #[test]
    fn spool_chunk_read_path_ignores_regular_reads() {
        let args = json!({
            "path": "src/main.rs",
            "offset": 1,
            "limit": 100
        });

        assert_eq!(spool_chunk_read_path(tools::READ_FILE, &args), None);
    }

    #[test]
    fn spool_chunk_read_path_ignores_non_read_unified_file_actions() {
        let args = json!({
            "action": "write",
            "path": ".vtcode/context/tool_outputs/unified_exec_789.txt",
            "content": "replacement"
        });

        assert_eq!(spool_chunk_read_path(tools::UNIFIED_FILE, &args), None);
    }
}
