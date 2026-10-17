use std::path::{Path, PathBuf};

use anyhow::Result;

use super::SkillLoaderConfig;
use super::discovery::{find_git_root, skill_roots_with_home_dir};
use crate::skills::cli_bridge::CliToolBridge;
use crate::skills::command_skills::{
    BuiltInCommandSkill, CommandSkillBackend, built_in_command_skill, find_command_skill_by_skill_name,
    merge_built_in_command_skill_contexts,
};
use crate::skills::container_validation::{
    ContainerSkillsValidator, ContainerValidationReport, ContainerValidationResult,
};
use crate::skills::discovery::{DiscoveryConfig, DiscoveryResult, SkillDiscovery};
use crate::skills::model::SkillScope;
use crate::skills::system::{install_system_skills, system_cache_root_dir};
use crate::skills::types::{Skill, SkillContext};
use crate::tools::error_messages::skill_ops;

/// Enhanced skill variant for unified handling
pub enum EnhancedSkill {
    /// Traditional instruction-based skill
    Traditional(Box<Skill>),
    /// CLI-based tool skill
    CliTool(Box<CliToolBridge>),
    /// Built-in VT Code command skill
    BuiltInCommand(Box<BuiltInCommandSkill>),
    /// Native code plugin skill reserved for explicitly gated integrations
    NativePlugin(Box<dyn crate::skills::native_plugin::NativePluginTrait>),
}

impl std::fmt::Debug for EnhancedSkill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Traditional(skill) => f.debug_tuple("Traditional").field(skill).finish(),
            Self::CliTool(tool) => f.debug_tuple("CliTool").field(tool).finish(),
            Self::BuiltInCommand(skill) => f.debug_tuple("BuiltInCommand").field(skill).finish(),
            Self::NativePlugin(plugin) => f.debug_tuple("NativePlugin").field(plugin).finish(),
        }
    }
}

/// High-level loader that provides discovery and validation features
pub struct EnhancedSkillLoader {
    workspace_root: PathBuf,
    codex_home: PathBuf,
    discovery: SkillDiscovery,
}

fn default_codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

pub(crate) fn discovery_config_for_codex_home(workspace_root: &Path, codex_home: &Path) -> DiscoveryConfig {
    let home_dir = dirs::home_dir();
    let loader_config = SkillLoaderConfig {
        codex_home: codex_home.to_path_buf(),
        cwd: workspace_root.to_path_buf(),
        project_root: find_git_root(workspace_root),
        include_bundled_system_skills: true,
    };
    let roots = skill_roots_with_home_dir(&loader_config, home_dir.as_deref());

    DiscoveryConfig {
        skill_paths: roots
            .iter()
            .filter(|root| !root.is_tool_root && !root.is_plugin_root)
            .map(|root| root.path.clone())
            .collect(),
        tool_paths: roots
            .iter()
            .filter(|root| root.is_tool_root)
            .map(|root| root.path.clone())
            .collect(),
        ..Default::default()
    }
}

impl EnhancedSkillLoader {
    /// Create a new enhanced loader for workspace
    pub fn new(workspace_root: PathBuf) -> Self {
        let codex_home = default_codex_home();
        let discovery = SkillDiscovery::with_config(discovery_config_for_codex_home(&workspace_root, &codex_home));
        Self { workspace_root, codex_home, discovery }
    }

    /// Create a loader pinned to a specific VT Code home directory.
    pub fn with_codex_home(workspace_root: PathBuf, codex_home: PathBuf) -> Self {
        let discovery = SkillDiscovery::with_config(discovery_config_for_codex_home(&workspace_root, &codex_home));
        Self { workspace_root, codex_home, discovery }
    }

    fn ensure_system_skills_installed(&self) {
        if let Err(err) = install_system_skills(&self.codex_home) {
            tracing::warn!("enhanced skill loader failed to install bundled system skills: {err}");
        }
    }

    /// Discover all available skills and tools
    pub async fn discover_all_skills(&mut self) -> Result<DiscoveryResult> {
        self.ensure_system_skills_installed();
        let mut result = self.discovery.discover_all(&self.workspace_root).await?;
        merge_built_in_command_skill_contexts(&mut result.skills);
        Ok(result)
    }

    /// Get a specific skill by name
    pub async fn get_skill(&mut self, name: &str) -> Result<EnhancedSkill> {
        self.ensure_system_skills_installed();
        let result = self.discovery.discover_all(&self.workspace_root).await?;

        // Try traditional skills first. Command skills (`cmd-*` with a bundled
        // `.system` contract) prefer the System copy so a stale workspace
        // shadow cannot break `/review` free-form + fix-mode behavior.
        if let Some(skill_ctx) = select_traditional_skill_ctx(&result.skills, name, &self.workspace_root) {
            let path = skill_ctx.path();
            let (manifest, instructions) = crate::skills::manifest::parse_skill_file(path)?;
            let skill = Skill::with_scope(
                manifest,
                path.clone(),
                infer_scope_from_skill_path(path, &self.workspace_root),
                instructions,
            )?;
            return Ok(EnhancedSkill::Traditional(Box::new(skill)));
        }

        // Try CLI tools
        for tool_config in &result.tools {
            if tool_config.name == name {
                let bridge = CliToolBridge::new(tool_config.clone())?;
                return Ok(EnhancedSkill::CliTool(Box::new(bridge)));
            }
        }

        if let Some(skill) = built_in_command_skill(name) {
            return Ok(EnhancedSkill::BuiltInCommand(Box::new(skill)));
        }

        // Native plugins are intentionally not loaded by the generic skill
        // lookup. Opening a dynamic library executes arbitrary native code
        // before this loader can inspect its metadata, so native loading must
        // remain an explicit, separately-gated operation.
        Err(skill_ops::skill_not_found_error(name))
    }

    /// Generate a comprehensive container validation report
    pub async fn generate_validation_report(&mut self) -> Result<ContainerValidationReport> {
        let result = self.discovery.discover_all(&self.workspace_root).await?;
        let mut report = ContainerValidationReport::new();
        let validator = ContainerSkillsValidator::new();

        for skill_ctx in &result.skills {
            match self.load_full_skill_from_ctx(skill_ctx) {
                Ok(skill) => {
                    let analysis = validator.analyze_skill(&skill);
                    report.add_skill_analysis(skill.name().to_string(), analysis);
                }
                Err(e) => {
                    report.add_incompatible_skill(
                        skill_ctx.manifest().name.clone(),
                        skill_ctx.manifest().description.clone(),
                        format!("Load error: {e}"),
                    );
                }
            }
        }

        report.finalize();
        Ok(report)
    }

    /// Check container requirements for a skill
    pub fn check_container_requirements(&self, skill: &Skill) -> ContainerValidationResult {
        let validator = ContainerSkillsValidator::new();
        validator.analyze_skill(skill)
    }

    fn load_full_skill_from_ctx(&self, ctx: &SkillContext) -> Result<Skill> {
        let path = ctx.path();
        let (manifest, instructions) = crate::skills::manifest::parse_skill_file(path)?;
        Skill::with_scope(manifest, path.clone(), infer_scope_from_skill_path(path, &self.workspace_root), instructions)
    }
}

fn infer_scope_from_skill_path(path: &Path, workspace_root: &Path) -> SkillScope {
    if path.starts_with(Path::new("/etc/codex/skills")) {
        return SkillScope::Admin;
    }
    if path.starts_with(system_cache_root_dir(&default_codex_home())) {
        return SkillScope::System;
    }
    if let Some(home) = dirs::home_dir()
        && path.starts_with(home.join(".agents/skills"))
    {
        return SkillScope::User;
    }
    if path.starts_with(workspace_root) {
        return SkillScope::Repo;
    }

    // Fallback for non-canonicalized paths: walk the components looking for a
    // `.agents/skills` ancestor. Uses proper component matching rather than
    // string-contains so a directory named `skills-backup` cannot trigger a
    // false positive.
    if has_agents_skills_ancestor(path) {
        return SkillScope::Repo;
    }

    SkillScope::User
}

fn has_agents_skills_ancestor(path: &Path) -> bool {
    let mut ancestors = path.parent().map(Path::new).into_iter().peekable();
    while let Some(current) = ancestors.next() {
        let is_skills = current.file_name().and_then(|f| f.to_str()) == Some("skills");
        let parent_is_agents = ancestors.peek().and_then(|p| p.file_name().and_then(|f| f.to_str())) == Some(".agents");
        if is_skills && parent_is_agents {
            return true;
        }
    }
    false
}

fn is_bundled_command_skill(name: &str) -> bool {
    find_command_skill_by_skill_name(name)
        .is_some_and(|spec| matches!(spec.backend, CommandSkillBackend::TraditionalSkill { .. }))
}

fn is_system_skill_path(path: &Path) -> bool {
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let is_skills = component.as_os_str() == "skills";
        let next_is_system = components.peek().is_some_and(|next| next.as_os_str() == ".system");
        if is_skills && next_is_system {
            return true;
        }
    }
    false
}

pub(crate) fn select_traditional_skill_ctx<'a>(
    skills: &'a [SkillContext],
    name: &str,
    workspace_root: &Path,
) -> Option<&'a SkillContext> {
    let mut first_match: Option<&'a SkillContext> = None;
    let mut system_match: Option<&'a SkillContext> = None;
    for skill_ctx in skills {
        if skill_ctx.manifest().name != name {
            continue;
        }
        if first_match.is_none() {
            first_match = Some(skill_ctx);
        }
        // Shape-based system detection covers custom `CODEX_HOME` values where
        // `infer_scope_from_skill_path` falls back to `User`. Exclude `Repo`
        // scope so a workspace-nested `skills/.system` path cannot spoof it.
        let scope = infer_scope_from_skill_path(skill_ctx.path(), workspace_root);
        if system_match.is_none()
            && (scope == SkillScope::System || (scope != SkillScope::Repo && is_system_skill_path(skill_ctx.path())))
        {
            system_match = Some(skill_ctx);
        }
        if first_match.is_some() && system_match.is_some() {
            break;
        }
    }
    if is_bundled_command_skill(name) {
        system_match.or(first_match)
    } else {
        first_match
    }
}
