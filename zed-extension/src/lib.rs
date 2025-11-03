use std::{env, path::PathBuf};

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

        let command = env::var("VT_CODE_BINARY").unwrap_or_else(|_| "vtcode".to_string());

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
