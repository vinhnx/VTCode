use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct FileAccessTarget {
    pub(super) path: PathBuf,
    pub(super) access_type: AccessType,
}

fn primary_path_arg(args: &Value) -> Option<&str> {
    args.get("path")
        .and_then(|value| value.as_str())
        .or_else(|| args.get("file_path").and_then(|value| value.as_str()))
        .or_else(|| args.get("filepath").and_then(|value| value.as_str()))
        .or_else(|| args.get("target_path").and_then(|value| value.as_str()))
}

fn destination_path_arg(args: &Value) -> Option<&str> {
    args.get("destination").and_then(|value| value.as_str())
}

fn push_file_access_target(targets: &mut Vec<FileAccessTarget>, path: &str, access_type: AccessType) {
    let path_str = path.trim();
    if path_str.is_empty() {
        return;
    }

    let path = PathBuf::from(path_str);
    // For small number of targets, linear search is faster than HashSet.
    // In large patches, we'll use a local HashSet in patch_file_access_targets.
    if targets
        .iter()
        .any(|existing| existing.path == path && existing.access_type == access_type)
    {
        return;
    }

    targets.push(FileAccessTarget { path, access_type });
}

pub(super) fn command_text_for_tool(tool_name: &str, args: &Value) -> Option<String> {
    match tool_name {
        tools::EXEC_COMMAND => crate::tools::command_args::command_text(args).ok().flatten(),
        tools::WRITE_STDIN => crate::tools::command_args::interactive_input_text(args).map(str::to_owned),
        tools::SHELL | tools::RUN_PTY_CMD => crate::tools::command_args::command_text(args).ok().flatten(),
        tools::SEND_PTY_INPUT => crate::tools::command_args::interactive_input_text(args).map(str::to_owned),
        tools::UNIFIED_EXEC if command_session_action_is(args, "run") => {
            crate::tools::command_args::command_text(args).ok().flatten()
        }
        tools::UNIFIED_EXEC if command_session_action_in(args, &["write", "continue"]) => {
            crate::tools::command_args::interactive_input_text(args).map(str::to_owned)
        }
        _ => None,
    }
}

pub(super) fn patch_file_access_targets(args: &Value) -> Vec<FileAccessTarget> {
    let Ok(Some(patch_input)) = decode_apply_patch_input(args) else {
        return Vec::new();
    };
    let Ok(patch) = Patch::parse(&patch_input.text) else {
        return Vec::new();
    };

    let mut targets = Vec::new();
    for operation in patch.operations() {
        match operation {
            PatchOperation::AddFile { path, .. } => {
                push_file_access_target(&mut targets, path, AccessType::Write);
            }
            PatchOperation::DeleteFile { path } => {
                push_file_access_target(&mut targets, path, AccessType::Delete);
            }
            PatchOperation::UpdateFile { path, new_path, .. } => {
                push_file_access_target(&mut targets, path, AccessType::Modify);
                if let Some(destination) = new_path.as_deref().filter(|candidate| *candidate != path) {
                    push_file_access_target(&mut targets, destination, AccessType::Write);
                }
            }
        }
    }

    targets
}

pub(super) fn file_access_targets(tool_name: &str, args: &Value) -> Vec<FileAccessTarget> {
    let mut targets = Vec::new();

    match tool_name {
        tools::WRITE_FILE | tools::CREATE_FILE => {
            if let Some(path) = primary_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Write);
            }
        }
        tools::EDIT_FILE | "search_replace" => {
            if let Some(path) = primary_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Modify);
            }
        }
        tools::DELETE_FILE => {
            if let Some(path) = primary_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Delete);
            }
        }
        tools::MOVE_FILE => {
            if let Some(path) = primary_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Modify);
            }
            if let Some(path) = destination_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Write);
            }
        }
        tools::COPY_FILE => {
            if let Some(path) = destination_path_arg(args) {
                push_file_access_target(&mut targets, path, AccessType::Write);
            }
        }
        tools::APPLY_PATCH => {
            targets.extend(patch_file_access_targets(args));
        }
        tools::UNIFIED_FILE => {
            // Single home of the unified-file mapping; mirrors the legacy
            // file-helper arms above. Keep them in sync.
            if file_operation_action_is(args, "write") {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Write);
                }
            } else if file_operation_action_is(args, "edit") {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Modify);
                }
            } else if file_operation_action_is(args, "delete") {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Delete);
                }
            } else if file_operation_action_is(args, "move") {
                if let Some(path) = primary_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Modify);
                }
                if let Some(path) = destination_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Write);
                }
            } else if file_operation_action_is(args, "copy") {
                if let Some(path) = destination_path_arg(args) {
                    push_file_access_target(&mut targets, path, AccessType::Write);
                }
            }
        }
        _ => {}
    }

    targets
}

pub(super) fn proposed_changes_preview(args: &Value) -> String {
    const PREVIEW_LIMIT: usize = 500;

    let preview_text = |label: &str, text: &str| {
        let prefix = vtcode_commons::formatting::truncate_utf8_prefix(text, PREVIEW_LIMIT);
        format!("{label} ({} bytes):\n{}{}", text.len(), prefix, if prefix.len() < text.len() { "..." } else { "" })
    };

    if let Some(content) = args.get("content").and_then(|value| value.as_str()) {
        return preview_text("Content", content);
    }

    if let Some(old_str) = args.get("old_str").and_then(|value| value.as_str()) {
        let new_str = args.get("new_str").and_then(|value| value.as_str()).unwrap_or("");
        // Bound like the content/patch branches: an unbounded replacement
        // would bloat the approval prompt and audit log.
        return format!(
            "Replace:\n  '{}'\nWith:\n  '{}'",
            vtcode_commons::formatting::truncate_byte_budget(old_str, PREVIEW_LIMIT, "..."),
            vtcode_commons::formatting::truncate_byte_budget(new_str, PREVIEW_LIMIT, "...")
        );
    }

    if let Ok(Some(patch_input)) = decode_apply_patch_input(args) {
        return preview_text("Patch", &patch_input.text);
    }

    "No details provided".to_string()
}
