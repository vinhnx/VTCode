use crate::skills::cli_bridge::{CliToolBridge, CliToolConfig, discover_cli_tools};
use crate::skills::model::{SkillErrorInfo, SkillLoadOutcome, SkillMetadata, SkillScope};
use crate::skills::system::system_cache_root_dir;
use anyhow::Result;
use dunce::canonicalize as normalize_path;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::error;

// Config for loader
#[derive(Debug, Clone)]
pub struct SkillLoaderConfig {
    pub codex_home: PathBuf,
    pub cwd: PathBuf,
    pub project_root: Option<PathBuf>,
}

pub struct SkillRoot {
    pub path: PathBuf,
    pub scope: SkillScope,
    pub is_tool_root: bool,
}

pub fn load_skills(config: &SkillLoaderConfig) -> SkillLoadOutcome {
    let mut outcome = SkillLoadOutcome::default();
    let roots = skill_roots(config);
    
    for root in roots {
        discover_skills_under_root(&root, &mut outcome);
    }

    // Auto-discover system CLI tools if needed (or we can skip this if we only want explicit paths)
    // vtcode's existing logic auto-discovers system tools.
    // We can add them as "System" scope skills.
    if let Ok(system_tools) = discover_cli_tools() {
        for tool in system_tools {
            if let Ok(skill) = tool_config_to_metadata(&tool, SkillScope::System) {
                outcome.skills.push(skill);
            }
        }
    }

    // Deduplicate by name
    let mut seen: HashSet<String> = HashSet::new();
    outcome.skills.retain(|skill| seen.insert(skill.name.clone()));

    // Sort
    outcome.skills.sort_by(|a, b| a.name.cmp(&b.name));

    outcome
}

fn skill_roots(config: &SkillLoaderConfig) -> Vec<SkillRoot> {
    let mut roots = Vec::new();

    // 1. Repo/Project roots (highest priority)
    // We check for .codex/skills, .vtcode/skills, etc.
    if let Some(project_root) = &config.project_root {
        // Traditional skills
        roots.push(SkillRoot {
            path: project_root.join(".codex/skills"),
            scope: SkillScope::Repo,
            is_tool_root: false,
        });
        roots.push(SkillRoot {
            path: project_root.join(".vtcode/skills"),
            scope: SkillScope::Repo,
            is_tool_root: false,
        });
        roots.push(SkillRoot {
            path: project_root.join("skills"),
            scope: SkillScope::Repo,
            is_tool_root: false,
        });
        
         // Tool roots
        roots.push(SkillRoot {
            path: project_root.join("tools"),
            scope: SkillScope::Repo,
            is_tool_root: true,
        });
        roots.push(SkillRoot {
            path: project_root.join("vendor/tools"),
            scope: SkillScope::Repo,
            is_tool_root: true,
        });
    }

    // 2. User roots
    roots.push(SkillRoot {
        path: config.codex_home.join("skills"),
        scope: SkillScope::User,
        is_tool_root: false,
    });
    roots.push(SkillRoot {
        path: config.codex_home.join("tools"),
        scope: SkillScope::User,
        is_tool_root: true,
    });

    // 3. System roots
    roots.push(SkillRoot {
        path: system_cache_root_dir(&config.codex_home),
        scope: SkillScope::System,
        is_tool_root: false,
    });

    roots
}

fn discover_skills_under_root(root: &SkillRoot, outcome: &mut SkillLoadOutcome) {
    let Ok(root_path) = normalize_path(&root.path) else {
        return;
    };

    if !root_path.is_dir() {
        return;
    }

    let mut queue: VecDeque<PathBuf> = VecDeque::from([root_path]);
    while let Some(dir) = queue.pop_front() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                error!("failed to read skills dir {}: {e:#}", dir.display());
                continue;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = match path.file_name().and_then(|f| f.to_str()) {
                Some(name) => name,
                None => continue,
            };

            if file_name.starts_with('.') {
                continue;
            }

            if path.is_dir() {
                queue.push_back(path.clone());
                
                // If this is a tool root or we are in a generic scan, check for tool directory structure
                // Assuming tool dir has tool.json or executable
                if root.is_tool_root {
                     if let Ok(Some(tool_meta)) = try_load_tool_from_dir(&path, root.scope) {
                         outcome.skills.push(tool_meta);
                     }
                }
                continue;
            }

            // Check for traditional skill
            if file_name == "SKILL.md" {
                match crate::skills::manifest::parse_skill_file(&path) {
                    Ok((manifest, _)) => {
                        outcome.skills.push(SkillMetadata {
                            name: manifest.name.clone(),
                            description: manifest.description.clone(),
                            short_description: None,
                            path: path.clone(),
                            scope: root.scope,
                            manifest: Some(manifest),
                        });
                    }
                    Err(err) => {
                        if root.scope != SkillScope::System {
                            outcome.errors.push(SkillErrorInfo {
                                path: path.clone(),
                                message: err.to_string(),
                            });
                        }
                    }
                }
            } else if root.is_tool_root && is_executable_file(&path) {
                 // Standalone executable tool?
                 // We typically look for directories, but maybe standalone files too.
                 // For now, let's stick to directory-based tools or tools with README.
            }
        }
    }
}

fn try_load_tool_from_dir(path: &Path, scope: SkillScope) -> Result<Option<SkillMetadata>> {
    // Check if it's a CLI tool directory (has tool.json or is executable inside)
    // Simplified: check for tool.json
    let tool_bridge = if path.join("tool.json").exists() {
         CliToolBridge::from_directory(path)?
    } else {
        // Heuristic: check for executable with same name as dir?
        // This is complex to reproduce exactly "discovery.rs" logic without code dupe.
        // I'll be conservative and require tool.json OR evident executable.
        match CliToolBridge::from_directory(path) {
            Ok(b) => b,
            Err(_) => return Ok(None),
        }
    };
    
    tool_config_to_metadata(&tool_bridge.config, scope).map(Some)
}

fn tool_config_to_metadata(config: &CliToolConfig, scope: SkillScope) -> Result<SkillMetadata> {
    Ok(SkillMetadata {
        name: config.name.clone(),
        description: config.description.clone(),
        short_description: None,
        path: config.executable_path.clone(), // Path to executable is the "path" of the skill? 
        // Or path to directory? Reference uses SKILL.md path.
        // Here we use executable path or tool directory.
        scope,
        manifest: None, // CLI tools don't have a manifest in the same sense, or we could synthesize one
    })
}

pub fn load_skill_resources(skill_path: &Path) -> Result<Vec<crate::skills::types::SkillResource>> {
    let mut resources = Vec::new();
    let resource_dir = skill_path.join("scripts");

    if resource_dir.exists() {
        for entry in fs::read_dir(&resource_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let rel_path = path
                    .strip_prefix(skill_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let resource_type = match path.extension().and_then(|e| e.to_str()) {
                    Some("py") | Some("sh") | Some("bash") => {
                        crate::skills::types::ResourceType::Script
                    }
                    Some("md") => crate::skills::types::ResourceType::Markdown,
                    Some("json") | Some("yaml") | Some("yml") => {
                        crate::skills::types::ResourceType::Reference
                    }
                    _ => crate::skills::types::ResourceType::Other(format!(
                        "{:?}",
                        path.extension()
                    )),
                };

                resources.push(crate::skills::types::SkillResource {
                    path: rel_path,
                    resource_type,
                    content: None,
                });
            }
        }
    }
    
    // Also check for references/ directory if needed, pattern is similar.
    Ok(resources)
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = path.metadata() {
            return meta.permissions().mode() & 0o111 != 0;
        }
    }
    #[cfg(windows)]
    {
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            return matches!(ext.to_lowercase().as_str(), "exe" | "bat" | "cmd");
        }
    }
    false
}
