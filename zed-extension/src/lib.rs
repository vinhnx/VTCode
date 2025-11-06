use std::{
    env,
    path::{Path, PathBuf},
};

use zed_extension_api as zed;

struct VTCodeExtension;

impl zed::Extension for VTCodeExtension {
    fn new() -> Self {
        Self
    }

    fn run_slash_command(
        &self,
        command: zed::SlashCommand,
        _args: Vec<String>,
        worktree: Option<&zed::Worktree>,
    ) -> zed::Result<zed::SlashCommandOutput> {
        match command.name.as_str() {
            "logs" => {
                let Some(worktree) = worktree else {
                    return Err("No workspace available".to_string());
                };
                let root = PathBuf::from(worktree.root_path());
                let log_paths = [
                    ("trajectory", ".vtcode/logs/trajectory.jsonl"),
                    ("sandbox", ".vtcode/logs/sandbox.jsonl"),
                    ("agent", ".vtcode/logs/agent.log"),
                ];

                let mut sections = Vec::with_capacity(log_paths.len());
                let mut text = String::new();

                for (label, relative) in log_paths {
                    let absolute = root.join(relative);
                    let absolute_str = absolute.display().to_string();
                    let start = text.len();
                    text.push_str(&absolute_str);
                    text.push('\n');

                    sections.push(zed::SlashCommandOutputSection {
                        range: (start..start + absolute_str.len()).into(),
                        label: format!("{} log", label),
                    });
                }

                Ok(zed::SlashCommandOutput { sections, text })
            }
            "status" => {
                let binary_hint = env::var("VT_CODE_BINARY")
                    .ok()
                    .unwrap_or_else(|| "vtcode".to_string());
                let mut text = String::from("VT Code Agent: ready\n");
                text.push_str("ACP Mode: enabled\n");
                text.push_str("Log Dir: .vtcode/logs\n");
                text.push_str(&format!("Binary: {}\n", binary_hint));

                Ok(zed::SlashCommandOutput {
                    sections: vec![zed::SlashCommandOutputSection {
                        range: (0..text.len()).into(),
                        label: "Status".to_string(),
                    }],
                    text,
                })
            }
            "doctor" => Ok(run_doctor(worktree)?),
            other => Err(format!("Unknown slash command: {other}")),
        }
    }

    fn context_server_command(
        &mut self,
        context_server_id: &zed::ContextServerId,
        project: &zed::Project,
    ) -> zed::Result<zed::Command> {
        if context_server_id.as_ref() != "vtcode" {
            return Err(format!("Unsupported context server: {context_server_id}"));
        }
        let _ = project;
        let command = resolve_vtcode_binary()?;

        Ok(zed::Command {
            command,
            args: vec!["acp".to_string()],
            env: vec![
                ("VT_ACP_ENABLED".to_string(), "1".to_string()),
                ("VT_ACP_ZED_ENABLED".to_string(), "1".to_string()),
                (
                    "VT_ACP_ZED_TOOLS_READ_FILE_ENABLED".to_string(),
                    "1".to_string(),
                ),
                (
                    "VT_ACP_ZED_TOOLS_LIST_FILES_ENABLED".to_string(),
                    "1".to_string(),
                ),
            ],
        })
    }
}

zed::register_extension!(VTCodeExtension);

fn resolve_vtcode_binary() -> zed::Result<String> {
    let (path, _) = locate_vtcode_binary()?;
    Ok(path)
}

fn validate_binary_path(path: &Path) -> bool {
    path.exists() && path.is_file()
}

fn normalize(path: PathBuf) -> String {
    if path.is_absolute() {
        path.display().to_string()
    } else {
        match std::env::current_dir() {
            Ok(current) => current.join(path).display().to_string(),
            Err(_) => path.display().to_string(),
        }
    }
}

fn locate_vtcode_binary() -> Result<(String, &'static str), String> {
    if let Ok(path) = env::var("VT_CODE_BINARY") {
        let candidate = PathBuf::from(&path);
        if validate_binary_path(&candidate) {
            return Ok((normalize(candidate), "VT_CODE_BINARY"));
        }

        return Err(format!(
            "VT_CODE_BINARY points to a missing binary: {}",
            path
        ));
    }

    let packaged_candidates = ["vtcode", "./vtcode", "bin/vtcode", "dist/vtcode"];
    for candidate in packaged_candidates {
        let path = PathBuf::from(candidate);
        if validate_binary_path(&path) {
            return Ok((normalize(path), candidate));
        }
    }

    Err("Unable to locate vtcode binary. Install the agent server targets or set VT_CODE_BINARY.".to_string())
}

fn run_doctor(worktree: Option<&zed::Worktree>) -> zed::Result<zed::SlashCommandOutput> {
    let mut text = String::new();
    let mut sections = Vec::new();

    push_report_line(&mut sections, &mut text, "Summary", "VT Code Doctor Report");
    text.push('\n');

    match locate_vtcode_binary() {
        Ok((path, source)) => {
            let line = format!("Binary: OK ({path}) [source: {source}]");
            push_report_line(&mut sections, &mut text, "Binary", &line);
        }
        Err(err) => {
            let line = format!("Binary: ERROR ({err})");
            push_report_line(&mut sections, &mut text, "Binary", &line);
        }
    }

    match worktree {
        Some(worktree) => {
            let root = PathBuf::from(worktree.root_path());
            let log_dir = root.join(".vtcode/logs");
            let line = if log_dir.exists() {
                format!("Logs: OK ({})", log_dir.display())
            } else {
                format!("Logs: MISSING ({})", log_dir.display())
            };
            push_report_line(&mut sections, &mut text, "Logs", &line);
        }
        None => {
            push_report_line(
                &mut sections,
                &mut text,
                "Logs",
                "Logs: UNKNOWN (open a workspace to verify)",
            );
        }
    }

    push_report_line(
        &mut sections,
        &mut text,
        "Context",
        "Context Server: enable VT Code under Settings -> Agents and ensure it is running",
    );

    Ok(zed::SlashCommandOutput { sections, text })
}

fn push_report_line(
    sections: &mut Vec<zed::SlashCommandOutputSection>,
    text: &mut String,
    label: &str,
    line: &str,
) {
    let start = text.len();
    text.push_str(line);
    text.push('\n');
    sections.push(zed::SlashCommandOutputSection {
        range: (start..start + line.len()).into(),
        label: label.to_string(),
    });
}
