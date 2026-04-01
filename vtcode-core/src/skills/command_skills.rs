use crate::skills::model::{SkillMetadata, SkillScope};
use crate::skills::types::{SkillContext, SkillManifest, SkillManifestMetadata, SkillVariety};
use hashbrown::HashMap;
use serde_json::json;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltInCommandExecutor {
    SlashAlias,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSkillBackend {
    TraditionalSkill {
        skill_name: &'static str,
        skill_path: &'static str,
    },
    BuiltInCommand {
        executor: BuiltInCommandExecutor,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSkillSpec {
    pub slash_name: &'static str,
    pub skill_name: &'static str,
    pub description: &'static str,
    pub usage: &'static str,
    pub category: &'static str,
    pub backend: CommandSkillBackend,
}

impl CommandSkillSpec {
    pub const fn is_traditional(self) -> bool {
        matches!(self.backend, CommandSkillBackend::TraditionalSkill { .. })
    }

    pub const fn is_built_in(self) -> bool {
        matches!(self.backend, CommandSkillBackend::BuiltInCommand { .. })
    }
}

#[derive(Debug, Clone)]
pub struct BuiltInCommandSkill {
    spec: &'static CommandSkillSpec,
    manifest: SkillManifest,
    path: PathBuf,
}

impl BuiltInCommandSkill {
    pub fn from_spec(spec: &'static CommandSkillSpec) -> Self {
        Self {
            spec,
            manifest: built_in_manifest(spec),
            path: built_in_path(spec.skill_name),
        }
    }

    pub fn spec(&self) -> &'static CommandSkillSpec {
        self.spec
    }

    pub fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    pub fn description(&self) -> &str {
        &self.manifest.description
    }

    pub fn usage(&self) -> &'static str {
        self.spec.usage
    }

    pub fn category(&self) -> &'static str {
        self.spec.category
    }

    pub fn slash_name(&self) -> &'static str {
        self.spec.slash_name
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn scope(&self) -> SkillScope {
        SkillScope::System
    }

    pub fn instructions(&self) -> String {
        format!(
            "# {}\n\nThis built-in command skill executes the existing `/{}` slash command backend.\n\n- Slash alias: `/{}`
\n- Usage: `{}`
\n- Category: `{}`
\n- Backend: `built_in`\n",
            self.name(),
            self.slash_name(),
            self.slash_name(),
            self.usage(),
            self.category()
        )
    }
}

macro_rules! built_in_command_spec {
    ($slash:literal, $description:literal, $usage:literal, $category:literal) => {
        CommandSkillSpec {
            slash_name: $slash,
            skill_name: concat!("cmd-", $slash),
            description: $description,
            usage: $usage,
            category: $category,
            backend: CommandSkillBackend::BuiltInCommand {
                executor: BuiltInCommandExecutor::SlashAlias,
            },
        }
    };
}

macro_rules! traditional_command_spec {
    ($slash:literal, $description:literal, $usage:literal, $category:literal, $skill_path:literal) => {
        CommandSkillSpec {
            slash_name: $slash,
            skill_name: concat!("cmd-", $slash),
            description: $description,
            usage: $usage,
            category: $category,
            backend: CommandSkillBackend::TraditionalSkill {
                skill_name: concat!("cmd-", $slash),
                skill_path: $skill_path,
            },
        }
    };
}

pub const COMMAND_SKILL_SPECS: &[CommandSkillSpec] = &[
    built_in_command_spec!(
        "init",
        "Create vtcode.toml and index the workspace (usage: /init [--force])",
        "/init [--force]",
        "workspace"
    ),
    built_in_command_spec!(
        "loop",
        "Run a session-scoped scheduled prompt repeatedly (usage: /loop [interval] <prompt>)",
        "/loop [interval] <prompt>",
        "automation"
    ),
    built_in_command_spec!(
        "schedule",
        "Manage durable scheduled tasks interactively (usage: /schedule)",
        "/schedule [list|create|delete ...]",
        "automation"
    ),
    built_in_command_spec!(
        "config",
        "Browse settings sections or focused memory controls (usage: /config [path|memory])",
        "/config [path|memory]",
        "configuration"
    ),
    built_in_command_spec!(
        "permissions",
        "Open the permissions settings section and effective summary",
        "/permissions",
        "configuration"
    ),
    built_in_command_spec!(
        "memory",
        "Show memory status, loaded AGENTS/rules, and quick memory actions",
        "/memory",
        "configuration"
    ),
    built_in_command_spec!(
        "vim",
        "Toggle Vim-style prompt editing (usage: /vim [on|off|toggle])",
        "/vim [on|off|toggle]",
        "configuration"
    ),
    built_in_command_spec!(
        "model",
        "Launch the interactive model picker",
        "/model",
        "configuration"
    ),
    built_in_command_spec!(
        "ide",
        "Toggle IDE context for this session",
        "/ide",
        "configuration"
    ),
    built_in_command_spec!(
        "theme",
        "Switch UI theme (usage: /theme <theme-id>)",
        "/theme [theme-id]",
        "configuration"
    ),
    traditional_command_spec!(
        "command",
        "Run a terminal command (usage: /command <program> [args...])",
        "/command <program> [args...]",
        "tools",
        ".system/cmd-command"
    ),
    built_in_command_spec!(
        "edit",
        "Open file in external editor (tools.editor config, then VISUAL/EDITOR) (usage: /edit [file])",
        "/edit [file]",
        "tools"
    ),
    built_in_command_spec!(
        "git",
        "Launch git interface (lazygit or interactive git)",
        "/git",
        "tools"
    ),
    traditional_command_spec!(
        "analyze",
        "Perform comprehensive codebase analysis and generate reports (usage: /analyze [full|security|performance])",
        "/analyze [full|security|performance]",
        "tools",
        ".system/cmd-analyze"
    ),
    traditional_command_spec!(
        "review",
        "Review the current diff or selected files (usage: /review [--last-diff|--target <expr>|--file <path>|files...] [--style <style>])",
        "/review [--last-diff|--target <expr>|--file <path>|files...] [--style <style>]",
        "tools",
        ".system/cmd-review"
    ),
    built_in_command_spec!(
        "files",
        "Browse and select files from workspace (usage: /files [filter])",
        "/files [filter]",
        "tools"
    ),
    built_in_command_spec!(
        "copy",
        "Copy the latest complete assistant reply to clipboard",
        "/copy",
        "tools"
    ),
    built_in_command_spec!(
        "suggest",
        "Suggest follow-up prompts from the current session context",
        "/suggest",
        "tools"
    ),
    built_in_command_spec!(
        "tasks",
        "Toggle the dedicated TODO panel fed by task_tracker output",
        "/tasks",
        "tools"
    ),
    built_in_command_spec!(
        "jobs",
        "Inspect active/background command sessions",
        "/jobs",
        "tools"
    ),
    built_in_command_spec!(
        "skills",
        "Open interactive skills manager (usage: /skills, /skills manager)",
        "/skills [manager|list|search|create|load|unload|info|use|validate|package|regenerate-index|help]",
        "tools"
    ),
    built_in_command_spec!(
        "agents",
        "Manage subagents and delegated child threads (usage: /agents [list|create [project|user] [name]|edit [name]|delete <name>|threads])",
        "/agents [list|threads|inspect <id>|close <id>|create [project|user] [name]|edit [name]|delete <name>]",
        "tools"
    ),
    built_in_command_spec!(
        "agent",
        "Show delegated child threads for the current session",
        "/agent [threads|inspect <id>|close <id>]",
        "tools"
    ),
    built_in_command_spec!(
        "subprocess",
        "Open local agents or manage background subprocesses (usage: /subprocess[es] [list|toggle|refresh|inspect <id>|stop <id>|cancel <id>])",
        "/subprocess[es] [list|toggle|refresh|inspect <id>|stop <id>|cancel <id>]",
        "tools"
    ),
    built_in_command_spec!(
        "status",
        "Show model, provider, workspace, and tool status",
        "/status",
        "status"
    ),
    built_in_command_spec!(
        "stop",
        "Stop the active turn immediately",
        "/stop",
        "status"
    ),
    built_in_command_spec!(
        "pause",
        "Pause the active turn at the next safe boundary",
        "/pause",
        "status"
    ),
    built_in_command_spec!(
        "doctor",
        "Run installation and configuration diagnostics (interactive in inline UI; usage: /doctor [--quick|--full])",
        "/doctor [--quick|--full]",
        "status"
    ),
    built_in_command_spec!(
        "update",
        "Check for new VT Code releases and install updates (usage: /update [check|install] [--force])",
        "/update [check|install] [--force]",
        "status"
    ),
    built_in_command_spec!(
        "mcp",
        "Open interactive MCP manager (usage: /mcp, optional subcommands still supported)",
        "/mcp [status|list|tools|refresh|config|config edit|repair|diagnose|login <name>|logout <name>]",
        "integration"
    ),
    built_in_command_spec!(
        "resume",
        "List archived sessions when idle; resume the active turn while it is paused",
        "/resume [limit|--all]",
        "session"
    ),
    built_in_command_spec!(
        "fork",
        "Fork an archived session into a new thread (usage: /fork [limit] [--all])",
        "/fork [limit] [--all]",
        "session"
    ),
    built_in_command_spec!(
        "history",
        "Open command history picker (usage: /history, same as Ctrl+R)",
        "/history",
        "session"
    ),
    built_in_command_spec!(
        "clear",
        "Clear visible screen (usage: /clear [new])",
        "/clear [new]",
        "session"
    ),
    built_in_command_spec!(
        "compact",
        "Open the OpenAI compact manager or run standalone /responses/compact with flags (usage: /compact [--instructions ...])",
        "/compact [--instructions <text>] [--max-output-tokens <n>] [--reasoning-effort <none|minimal|low|medium|high|xhigh>] [--verbosity <low|medium|high>] [--include <selector> ...] [--store|--no-store] [--service-tier <flex|priority>] [--prompt-cache-key <key>]",
        "session"
    ),
    built_in_command_spec!("new", "Start a new session", "/new", "session"),
    built_in_command_spec!(
        "share-log",
        "Export current session log as JSON or Markdown (usage: /share-log [json|markdown], alias: /export-log)",
        "/share-log [json|markdown]",
        "session"
    ),
    built_in_command_spec!(
        "rewind",
        "Open the rewind picker or restore a specific checkpoint (usage: /rewind [turn] [conversation|code|both])",
        "/rewind [turn] [conversation|code|both]",
        "session"
    ),
    built_in_command_spec!(
        "plan",
        "Plan Mode: read-only planning with optional prompt (usage: /plan [on|off] [task])",
        "/plan [on|off] [task]",
        "session"
    ),
    built_in_command_spec!(
        "mode",
        "Open the session mode picker or switch directly (usage: /mode [edit|auto|plan|cycle])",
        "/mode [edit|auto|plan|cycle]",
        "session"
    ),
    built_in_command_spec!(
        "docs",
        "Open vtcode documentation in web browser",
        "/docs",
        "support"
    ),
    built_in_command_spec!(
        "help",
        "Show slash command help",
        "/help [command]",
        "support"
    ),
    built_in_command_spec!("exit", "Exit the session", "/exit", "session"),
    built_in_command_spec!(
        "donate",
        "Support the project by buying the author a coffee",
        "/donate",
        "support"
    ),
    built_in_command_spec!(
        "terminal-setup",
        "Configure terminal for VT Code (multiline, copy/paste, shell, themes)",
        "/terminal-setup",
        "terminal"
    ),
    built_in_command_spec!(
        "statusline",
        "Set up a custom status line with target selection (usage: /statusline [instructions...])",
        "/statusline [instructions...]",
        "terminal"
    ),
    built_in_command_spec!(
        "login",
        "Authenticate with OpenAI, OpenRouter, or GitHub Copilot (usage: /login [provider])",
        "/login [provider]",
        "auth"
    ),
    built_in_command_spec!(
        "logout",
        "Clear stored provider authentication (usage: /logout [provider])",
        "/logout [provider]",
        "auth"
    ),
    built_in_command_spec!(
        "auth",
        "Show authentication status for providers (usage: /auth [provider])",
        "/auth [provider]",
        "auth"
    ),
    built_in_command_spec!(
        "refresh-oauth",
        "Refresh stored provider credentials when supported (usage: /refresh-oauth [provider])",
        "/refresh-oauth [provider]",
        "auth"
    ),
];

pub fn command_skill_specs() -> &'static [CommandSkillSpec] {
    COMMAND_SKILL_SPECS
}

pub fn find_command_skill_by_slash_name(name: &str) -> Option<&'static CommandSkillSpec> {
    COMMAND_SKILL_SPECS
        .iter()
        .find(|spec| spec.slash_name == name)
}

pub fn find_command_skill_by_skill_name(name: &str) -> Option<&'static CommandSkillSpec> {
    COMMAND_SKILL_SPECS
        .iter()
        .find(|spec| spec.skill_name == name)
}

pub fn is_command_skill_name(name: &str) -> bool {
    find_command_skill_by_skill_name(name).is_some()
}

pub fn is_model_catalog_eligible(skill: &SkillMetadata) -> bool {
    if skill
        .manifest
        .as_ref()
        .and_then(|manifest| manifest.disable_model_invocation)
        .unwrap_or(false)
    {
        return false;
    }

    !is_command_skill_name(&skill.name)
}

pub fn built_in_command_skill_contexts() -> Vec<SkillContext> {
    COMMAND_SKILL_SPECS
        .iter()
        .copied()
        .filter(|spec| spec.is_built_in())
        .map(|spec| {
            SkillContext::MetadataOnly(built_in_manifest(&spec), built_in_path(spec.skill_name))
        })
        .collect()
}

pub fn built_in_command_skill(name: &str) -> Option<BuiltInCommandSkill> {
    find_command_skill_by_skill_name(name)
        .filter(|spec| spec.is_built_in())
        .map(BuiltInCommandSkill::from_spec)
}

pub fn merge_built_in_command_skill_contexts(skills: &mut Vec<SkillContext>) {
    skills.extend(built_in_command_skill_contexts());
    skills.sort_by(|left, right| left.manifest().name.cmp(&right.manifest().name));
    skills.dedup_by(|left, right| left.manifest().name == right.manifest().name);
}

pub fn merge_built_in_command_skill_metadata(skills: &mut Vec<SkillMetadata>) {
    skills.extend(
        built_in_command_skill_contexts()
            .into_iter()
            .map(|skill_ctx| SkillMetadata {
                name: skill_ctx.manifest().name.clone(),
                description: skill_ctx.manifest().description.clone(),
                short_description: None,
                path: skill_ctx.path().clone(),
                scope: SkillScope::System,
                manifest: Some(skill_ctx.manifest().clone()),
            }),
    );
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    skills.dedup_by(|left, right| left.name == right.name);
}

fn built_in_manifest(spec: &CommandSkillSpec) -> SkillManifest {
    SkillManifest {
        name: spec.skill_name.to_string(),
        description: spec.description.to_string(),
        disable_model_invocation: Some(true),
        variety: SkillVariety::BuiltIn,
        metadata: Some(command_skill_metadata(spec, "built_in_command")),
        ..Default::default()
    }
}

fn command_skill_metadata(spec: &CommandSkillSpec, backend: &str) -> SkillManifestMetadata {
    let mut metadata = HashMap::new();
    metadata.insert(
        "slash_alias".to_string(),
        json!(format!("/{}", spec.slash_name)),
    );
    metadata.insert("usage".to_string(), json!(spec.usage));
    metadata.insert("category".to_string(), json!(spec.category));
    metadata.insert("backend".to_string(), json!(backend));
    metadata
}

fn built_in_path(skill_name: &str) -> PathBuf {
    PathBuf::from(format!("<built-in>/{}", skill_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traditional_and_built_in_commands_are_mapped() {
        let review = find_command_skill_by_slash_name("review").expect("review spec");
        assert!(review.is_traditional());
        assert_eq!(review.skill_name, "cmd-review");

        let status = find_command_skill_by_slash_name("status").expect("status spec");
        assert!(status.is_built_in());
        assert_eq!(status.skill_name, "cmd-status");
    }

    #[test]
    fn built_in_contexts_are_tagged_correctly() {
        let built_in = built_in_command_skill_contexts();
        let status = built_in
            .iter()
            .find(|ctx| ctx.manifest().name == "cmd-status")
            .expect("cmd-status context");
        assert_eq!(status.manifest().variety, SkillVariety::BuiltIn);
    }

    #[test]
    fn removed_generate_agent_file_command_is_not_registered() {
        assert!(find_command_skill_by_slash_name("generate-agent-file").is_none());
        assert!(find_command_skill_by_skill_name("cmd-generate-agent-file").is_none());
    }
}
