use super::*;

pub fn instruction_scope_label(scope: &InstructionScope) -> &'static str {
    match scope {
        InstructionScope::User => "user",
        InstructionScope::Workspace => "workspace",
        InstructionScope::Custom => "custom",
    }
}

pub fn instruction_source_label(source: &InstructionSource) -> String {
    match source.kind {
        InstructionSourceKind::Agents => {
            format!("{} {}", instruction_scope_label(&source.scope), instruction_file_label(&source.path),)
        }
        InstructionSourceKind::Extra => {
            format!("{} extra instructions", instruction_scope_label(&source.scope))
        }
        InstructionSourceKind::Rule if source.matched => {
            format!("{} matched rule", instruction_scope_label(&source.scope))
        }
        InstructionSourceKind::Rule => {
            format!("{} rule", instruction_scope_label(&source.scope))
        }
    }
}

pub(super) fn instruction_file_label(path: &Path) -> &'static str {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(CLAUDE_FILENAME))
    {
        "CLAUDE"
    } else {
        "AGENTS"
    }
}

pub fn format_instruction_path(path: &Path, project_root: &Path, home_dir: Option<&Path>) -> String {
    if let Ok(relative) = path.strip_prefix(project_root) {
        let display = relative.display().to_string();
        if !display.is_empty() {
            return display;
        }

        if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
            return name.to_string();
        }
    }

    if let Some(home) = home_dir
        && let Ok(relative) = path.strip_prefix(home)
    {
        let display = relative.display().to_string();
        if display.is_empty() {
            return "~".to_string();
        }

        return format!("~/{display}");
    }

    path.display().to_string()
}
