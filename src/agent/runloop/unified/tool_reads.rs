use serde_json::Value;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::tool_intent;

pub(crate) fn read_file_path_arg(args: &Value) -> Option<&str> {
    let obj = args.as_object()?;
    for key in ["path", "file_path", "filepath", "target_path", "file"] {
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
        tool_names::UNIFIED_FILE => tool_intent::file_operation_action(args)
            .unwrap_or("read")
            .eq_ignore_ascii_case("read"),
        _ => false,
    }
}

fn looks_like_tool_output_spool_path(path: &str) -> bool {
    vtcode_core::tools::SpooledOutputReference::recognizes_file_path(std::path::Path::new(path))
}

pub(crate) fn spool_chunk_read_path<'a>(canonical_tool_name: &str, args: &'a Value) -> Option<&'a str> {
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

/// Shell commands that page through a file without transforming it (or with a
/// line filter). Mirrors the read-only allowlist shape used by the read
/// family-key tracker so spool paging is recognized no matter which channel
/// the model uses.
const SPOOL_PAGE_SHELL_COMMANDS: &[&str] = &["sed", "cat", "head", "tail", "bat", "rg", "grep"];

/// Detect a shell paged read of a tool-output spool file (for example
/// `sed -n '320,520p' .vtcode/context/tool_outputs/...` or
/// `rg -n pattern <spool>`). Read-file-style spool reads are covered by
/// [`spool_chunk_read_path`]; this covers the exec channel, which is how
/// models usually page spools.
pub(crate) fn exec_spool_page_source(canonical_tool_name: &str, args: &Value) -> Option<String> {
    if !matches!(canonical_tool_name, tool_names::UNIFIED_EXEC | tool_names::EXEC_COMMAND | "command_session") {
        return None;
    }
    let words = vtcode_core::tools::command_args::command_words(args).ok()??;
    // Sequenced commands (`&&`/`||`/`;`) can piggyback a non-spool read onto a
    // spool page, letting non-spool content ride the paging exemption and its
    // preview credit after the aggregate budget is exhausted. Pipes are still
    // allowed so `cat spool | head` keeps working.
    if words.iter().any(|word| {
        matches!(word.as_str(), "&&" | "||" | ";") || word.ends_with(';') || word.contains("&&") || word.contains("||")
    }) {
        return None;
    }
    // Strip `env` prefixes and leading `VAR=val` assignments so `env sed ...`
    // is recognized like `sed ...`, mirroring verification classification.
    let words_after_env = vtcode_core::tools::command_args::command_words_after_environment_prefix(&words);
    let first = words_after_env.first()?;
    let base = first
        .rsplit('/')
        .next()
        .unwrap_or(first)
        .trim_matches(|ch| ch == '\'' || ch == '"')
        .to_ascii_lowercase();
    if !SPOOL_PAGE_SHELL_COMMANDS.contains(&base.as_str()) {
        return None;
    }
    words
        .iter()
        .find(|word| looks_like_tool_output_spool_path(word.as_str()))
        .cloned()
}

/// Unified spool-page detector across both read channels. Returns the source
/// spool path for read-file-style spool reads and shell paged reads of spool
/// files. Used by the spool-chunk guard (sequential cap) and the preview
/// budget (paging credit) so both channels share one definition of paging.
pub(crate) fn spool_page_source_path(canonical_tool_name: &str, args: &Value) -> Option<String> {
    spool_chunk_read_path(canonical_tool_name, args)
        .map(str::to_string)
        .or_else(|| exec_spool_page_source(canonical_tool_name, args))
}

/// Read the first 4 KB of a file to detect error payloads written by previous
/// tool calls. Used to short-circuit repeated reads of an error spool.
pub(crate) async fn read_spool_head_for_error_check(workspace: &std::path::Path, path: &str) -> Option<String> {
    let workspace = workspace.to_path_buf();
    let path = path.to_string();
    tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        let reference = vtcode_core::tools::SpooledOutputReference {
            spool_path: &path,
            preview: None,
            original_bytes: None,
            sha256: None,
            state: vtcode_core::tools::SpoolState::Pending,
        };
        reference.read_pending_bounded(&workspace, 4096)
    })
    .await
    .ok()?
    .ok()
    .filter(|head| !head.is_empty())
}

/// Returns `true` if the spool file's first bytes look like a tool error
/// response rather than a successful tool output. We check both:
///   - JSON `{"error":...}` envelopes produced by `build_error_content`
///   - Plain-text error fragments commonly written by outline / search tools
pub(crate) fn spool_content_looks_like_error(content: &str) -> bool {
    let trimmed = content.trim_start();
    if trimmed.starts_with("{\"error\"") {
        return true;
    }
    if trimmed.starts_with("{\"output\":\"Error") {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("error:") || lower.starts_with("tool error:")
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vtcode_core::config::constants::tools;

    use super::{
        exec_spool_page_source, spool_chunk_read_path, spool_content_looks_like_error, spool_page_source_path,
    };

    #[test]
    fn spool_path_requires_components_not_substrings() {
        for path in [
            "fake.vtcode/context/tool_outputs/file",
            ".vtcode/context/tool_outputs/../secret",
            ".vtcode/context/tool_outputs",
        ] {
            assert_eq!(spool_chunk_read_path(tools::READ_FILE, &json!({"path": path})), None);
        }
    }

    #[test]
    fn spool_chunk_read_path_matches_read_file_spool_reads() {
        let args = json!({
            "path": ".vtcode/context/tool_outputs/command_session_123.txt",
            "offset": 41,
            "limit": 40
        });

        assert_eq!(
            spool_chunk_read_path(tools::READ_FILE, &args),
            Some(".vtcode/context/tool_outputs/command_session_123.txt")
        );
    }

    #[test]
    fn spool_chunk_read_path_matches_file_operation_read_spool_reads() {
        let args = json!({
            "action": "read",
            "path": ".vtcode/context/tool_outputs/command_session_456.txt",
            "offset": 81,
            "limit": 40
        });

        assert_eq!(
            spool_chunk_read_path(tools::UNIFIED_FILE, &args),
            Some(".vtcode/context/tool_outputs/command_session_456.txt")
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
    fn spool_chunk_read_path_ignores_non_read_file_operation_actions() {
        let args = json!({
            "action": "write",
            "path": ".vtcode/context/tool_outputs/command_session_789.txt",
            "content": "replacement"
        });

        assert_eq!(spool_chunk_read_path(tools::UNIFIED_FILE, &args), None);
    }

    #[test]
    fn spool_content_looks_like_error_detects_json_error_envelope() {
        let payload = r#"{"error":"Spool chunk reads exceeded","failure_kind":"spool_chunk_guard"}"#;
        assert!(spool_content_looks_like_error(payload));
    }

    #[test]
    fn spool_content_looks_like_error_detects_outline_ast_grep_failure() {
        let payload = r#"{"output":"Error: search backend unavailable"}"#;
        assert!(spool_content_looks_like_error(payload));
    }

    #[test]
    fn spool_content_looks_like_error_detects_plain_error_prefix() {
        assert!(spool_content_looks_like_error("Error: file not found"));
        assert!(spool_content_looks_like_error("error: search backend unavailable"));
    }

    #[test]
    fn spool_content_looks_like_error_returns_false_for_normal_output() {
        assert!(!spool_content_looks_like_error("approval_recorder.rs\nassembly.rs\nbuilder.rs"));
        assert!(!spool_content_looks_like_error("=== mod.rs ===\npub struct ToolRegistry"));
    }

    #[test]
    fn exec_spool_page_source_matches_sed_range_on_spool() {
        let args = json!({"cmd": "sed -n '320,520p' .vtcode/context/tool_outputs/write_stdin_run-abc123.txt"});
        assert_eq!(
            exec_spool_page_source(tools::EXEC_COMMAND, &args),
            Some(".vtcode/context/tool_outputs/write_stdin_run-abc123.txt".to_string())
        );
    }

    #[test]
    fn exec_spool_page_source_matches_rg_on_spool() {
        let args = json!({"cmd": "rg -n StartupContext .vtcode/context/tool_outputs/write_stdin_run-abc123.txt"});
        assert_eq!(
            exec_spool_page_source(tools::UNIFIED_EXEC, &args),
            Some(".vtcode/context/tool_outputs/write_stdin_run-abc123.txt".to_string())
        );
    }

    #[test]
    fn exec_spool_page_source_ignores_non_spool_and_non_paging_commands() {
        let sed_source = json!({"cmd": "sed -n '1,80p' src/main.rs"});
        assert_eq!(exec_spool_page_source(tools::EXEC_COMMAND, &sed_source), None);
        let check = json!({"cmd": "cargo check --locked"});
        assert_eq!(exec_spool_page_source(tools::EXEC_COMMAND, &check), None);
        let read = json!({"path": ".vtcode/context/tool_outputs/write_stdin_run-abc123.txt"});
        assert_eq!(exec_spool_page_source(tools::READ_FILE, &read), None);
    }

    #[test]
    fn exec_spool_page_source_rejects_sequenced_piggyback_commands() {
        // A sequenced non-spool read must not ride the spool-paging exemption
        // (and its preview credit) after the aggregate budget is exhausted.
        for cmd in [
            "sed -n '1,80p' .vtcode/context/tool_outputs/write_stdin_run-abc123.txt && cat src/main.rs",
            "cat .vtcode/context/tool_outputs/write_stdin_run-abc123.txt; cat src/main.rs",
            "cat .vtcode/context/tool_outputs/write_stdin_run-abc123.txt || cat src/main.rs",
        ] {
            assert_eq!(
                exec_spool_page_source(tools::EXEC_COMMAND, &json!({"cmd": cmd})),
                None,
                "sequenced command must not count as spool paging: {cmd}"
            );
        }
        // Piped paging of the spool itself stays a spool page.
        let piped = json!({"cmd": "cat .vtcode/context/tool_outputs/write_stdin_run-abc123.txt | head -40"});
        assert_eq!(
            exec_spool_page_source(tools::EXEC_COMMAND, &piped),
            Some(".vtcode/context/tool_outputs/write_stdin_run-abc123.txt".to_string())
        );
    }

    #[test]
    fn exec_spool_page_source_strips_env_prefix() {
        let args = json!({"cmd": "env sed -n '1,80p' .vtcode/context/tool_outputs/write_stdin_run-abc123.txt"});
        assert_eq!(
            exec_spool_page_source(tools::EXEC_COMMAND, &args),
            Some(".vtcode/context/tool_outputs/write_stdin_run-abc123.txt".to_string())
        );
    }

    #[test]
    fn spool_page_source_path_unifies_both_channels() {
        let read = json!({"path": ".vtcode/context/tool_outputs/write_stdin_run-abc123.txt"});
        assert_eq!(
            spool_page_source_path(tools::READ_FILE, &read),
            Some(".vtcode/context/tool_outputs/write_stdin_run-abc123.txt".to_string())
        );
        let sed = json!({"cmd": "sed -n '1,80p' .vtcode/context/tool_outputs/write_stdin_run-abc123.txt"});
        assert_eq!(
            spool_page_source_path(tools::EXEC_COMMAND, &sed),
            Some(".vtcode/context/tool_outputs/write_stdin_run-abc123.txt".to_string())
        );
        let other = json!({"cmd": "sed -n '1,80p' src/main.rs"});
        assert_eq!(spool_page_source_path(tools::EXEC_COMMAND, &other), None);
    }
}
