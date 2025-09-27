use anyhow::{Context, Result, anyhow};
use regex::Regex;
use serde_json::{Value, json};
use shell_words::split;

use crate::config::constants::tools;
use crate::config::loader::ConfigManager;
use crate::tools::grep_search::GrepSearchResult;
use crate::tools::types::EditInput;

use super::ToolRegistry;
use super::utils;

impl ToolRegistry {
    pub async fn read_file(&mut self, args: Value) -> Result<Value> {
        self.execute_tool(tools::READ_FILE, args).await
    }

    pub async fn write_file(&mut self, args: Value) -> Result<Value> {
        self.execute_tool(tools::WRITE_FILE, args).await
    }

    pub async fn edit_file(&mut self, args: Value) -> Result<Value> {
        let input: EditInput = serde_json::from_value(args).context("invalid edit_file args")?;

        let read_args = json!({
            "path": input.path,
            "max_lines": 1000000
        });

        let read_result = self.file_ops_tool.read_file(read_args).await?;
        let current_content = read_result["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Failed to read file content"))?;

        let mut replacement_occurred = false;
        let mut new_content = current_content.to_string();

        if current_content.contains(&input.old_str) {
            new_content = current_content.replace(&input.old_str, &input.new_str);
            replacement_occurred = new_content != current_content;
        }

        if !replacement_occurred {
            let normalized_content = utils::normalize_whitespace(current_content);
            let normalized_old_str = utils::normalize_whitespace(&input.old_str);

            if normalized_content.contains(&normalized_old_str) {
                let old_lines: Vec<&str> = input.old_str.lines().collect();
                let content_lines: Vec<&str> = current_content.lines().collect();

                for i in 0..=(content_lines.len().saturating_sub(old_lines.len())) {
                    let window = &content_lines[i..i + old_lines.len()];
                    if utils::lines_match(window, &old_lines) {
                        let before = content_lines[..i].join("\n");
                        let after = content_lines[i + old_lines.len()..].join("\n");
                        let replacement_lines: Vec<&str> = input.new_str.lines().collect();

                        new_content =
                            format!("{}\n{}\n{}", before, replacement_lines.join("\n"), after);
                        replacement_occurred = true;
                        break;
                    }
                }
            }
        }

        if !replacement_occurred {
            let content_preview = if current_content.len() > 500 {
                format!(
                    "{}...{}",
                    &current_content[..250],
                    &current_content[current_content.len().saturating_sub(250)..]
                )
            } else {
                current_content.to_string()
            };

            return Err(anyhow!(
                "Could not find text to replace in file.\n\nExpected to replace:\n{}\n\nFile content preview:\n{}",
                input.old_str,
                content_preview
            ));
        }

        let write_args = json!({
            "path": input.path,
            "content": new_content,
            "mode": "overwrite"
        });

        self.file_ops_tool.write_file(write_args).await
    }

    pub async fn delete_file(&mut self, _args: Value) -> Result<Value> {
        Err(anyhow!("delete_file not yet implemented in modular system"))
    }

    pub async fn rp_search(&mut self, args: Value) -> Result<Value> {
        self.execute_tool(tools::GREP_SEARCH, args).await
    }

    pub fn last_rp_search_result(&self) -> Option<GrepSearchResult> {
        self.grep_search.last_result()
    }

    pub async fn list_files(&mut self, args: Value) -> Result<Value> {
        self.execute_tool(tools::LIST_FILES, args).await
    }

    pub async fn run_terminal_cmd(&mut self, args: Value) -> Result<Value> {
        let cfg = ConfigManager::load()
            .or_else(|_| ConfigManager::load_from_workspace("."))
            .or_else(|_| ConfigManager::load_from_file("vtcode.toml"))
            .map(|cm| cm.config().clone())
            .unwrap_or_default();

        let mut args = args;
        if let Some(cmd_str) = args.get("command").and_then(|v| v.as_str()) {
            let parts = split(cmd_str).context("failed to parse command string")?;
            if parts.is_empty() {
                return Err(anyhow!("command cannot be empty"));
            }
            if let Some(map) = args.as_object_mut() {
                map.insert("command".to_string(), json!(parts));
            }
        }

        let cmd_text = if let Some(cmd_val) = args.get("command") {
            if cmd_val.is_array() {
                cmd_val
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                cmd_val.as_str().unwrap_or("").to_string()
            }
        } else {
            String::new()
        };

        let mut deny_regex = cfg.commands.deny_regex.clone();
        if let Ok(extra) = std::env::var("VTCODE_COMMANDS_DENY_REGEX") {
            deny_regex.extend(extra.split(',').map(|s| s.trim().to_string()));
        }
        for pat in &deny_regex {
            if Regex::new(pat)
                .ok()
                .map(|re| re.is_match(&cmd_text))
                .unwrap_or(false)
            {
                return Err(anyhow!("Command denied by regex policy: {}", pat));
            }
        }
        let mut deny_glob = cfg.commands.deny_glob.clone();
        if let Ok(extra) = std::env::var("VTCODE_COMMANDS_DENY_GLOB") {
            deny_glob.extend(extra.split(',').map(|s| s.trim().to_string()));
        }
        for pat in &deny_glob {
            let re = format!("^{}$", regex::escape(pat).replace(r"\\*", ".*"));
            if Regex::new(&re)
                .ok()
                .map(|re| re.is_match(&cmd_text))
                .unwrap_or(false)
            {
                return Err(anyhow!("Command denied by glob policy: {}", pat));
            }
        }
        let mut deny_list = cfg.commands.deny_list.clone();
        if let Ok(extra) = std::env::var("VTCODE_COMMANDS_DENY_LIST") {
            deny_list.extend(extra.split(',').map(|s| s.trim().to_string()));
        }
        for d in &deny_list {
            if cmd_text.starts_with(d) {
                return Err(anyhow!("Command denied by policy: {}", d));
            }
        }

        let mut allow_regex = cfg.commands.allow_regex.clone();
        if let Ok(extra) = std::env::var("VTCODE_COMMANDS_ALLOW_REGEX") {
            allow_regex.extend(extra.split(',').map(|s| s.trim().to_string()));
        }
        let mut allow_glob = cfg.commands.allow_glob.clone();
        if let Ok(extra) = std::env::var("VTCODE_COMMANDS_ALLOW_GLOB") {
            allow_glob.extend(extra.split(',').map(|s| s.trim().to_string()));
        }
        let mut allow_ok = allow_regex.is_empty() && allow_glob.is_empty();
        if !allow_ok {
            if allow_regex.iter().any(|pat| {
                Regex::new(pat)
                    .ok()
                    .map(|re| re.is_match(&cmd_text))
                    .unwrap_or(false)
            }) {
                allow_ok = true;
            }
            if !allow_ok
                && allow_glob.iter().any(|pat| {
                    let re = format!("^{}$", regex::escape(pat).replace(r"\\*", ".*"));
                    Regex::new(&re)
                        .ok()
                        .map(|re| re.is_match(&cmd_text))
                        .unwrap_or(false)
                })
            {
                allow_ok = true;
            }
        }
        if !allow_ok {
            let mut allow_list = cfg.commands.allow_list.clone();
            if let Ok(extra) = std::env::var("VTCODE_COMMANDS_ALLOW_LIST") {
                allow_list.extend(extra.split(',').map(|s| s.trim().to_string()));
            }
            if !allow_list.is_empty() {
                allow_ok = allow_list.iter().any(|p| cmd_text.starts_with(p));
            }
        }
        if !allow_ok {
            return Err(anyhow!("Command not allowed by policy"));
        }

        if args.get("cwd").is_none() {
            if let Some(m) = args.as_object_mut() {
                m.insert(
                    "cwd".to_string(),
                    json!(self.workspace_root.display().to_string()),
                );
            }
        }

        if args.get("mode").is_none() {
            if let Some(m) = args.as_object_mut() {
                m.insert("mode".to_string(), json!("pty"));
            }
        }

        self.execute_tool(tools::RUN_TERMINAL_CMD, args).await
    }
}
