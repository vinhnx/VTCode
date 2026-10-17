use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::{error, warn};

use super::resources::is_executable_file;
use super::{SkillLoaderConfig, SkillRoot};
use crate::skills::cli_bridge::{CliToolBridge, CliToolConfig};
use crate::skills::locations::{MAX_DISCOVERY_DEPTH, MAX_DISCOVERY_DIRS, discovery_dir_skipped};
use crate::skills::model::{SkillErrorInfo, SkillLoadOutcome, SkillMetadata, SkillScope};
use crate::skills::system::system_cache_root_dir;
use vtcode_agent_plugins::LoadedPlugin;
use vtcode_commons::VtCodePaths;

pub(crate) fn skill_roots_with_home_dir(config: &SkillLoaderConfig, home_dir: Option<&Path>) -> Vec<SkillRoot> {
    let mut roots = Vec::new();

    for repo_dir in repo_skill_search_dirs(config) {
        roots.push(SkillRoot {
            path: repo_dir.join(".agents/skills"),
            scope: SkillScope::Repo,
            is_tool_root: false,
            is_plugin_root: false,
        });
    }

    if let Some(project_root) = &config.project_root {
        roots.push(SkillRoot {
            path: project_root.join(".agents/plugins"),
            scope: SkillScope::Repo,
            is_tool_root: false,
            is_plugin_root: true,
        });
        roots.push(SkillRoot {
            path: project_root.join("tools"),
            scope: SkillScope::Repo,
            is_tool_root: true,
            is_plugin_root: false,
        });
        roots.push(SkillRoot {
            path: project_root.join("vendor/tools"),
            scope: SkillScope::Repo,
            is_tool_root: true,
            is_plugin_root: false,
        });
    }

    if let Some(home) = home_dir {
        roots.push(SkillRoot {
            path: home.join(".agents/plugins"),
            scope: SkillScope::User,
            is_tool_root: false,
            is_plugin_root: true,
        });
        roots.push(SkillRoot {
            path: home.join(".agents/skills"),
            scope: SkillScope::User,
            is_tool_root: false,
            is_plugin_root: false,
        });

        if let Ok(paths) = VtCodePaths::resolve()
            && dirs::home_dir().as_deref() == Some(home)
        {
            roots.push(SkillRoot {
                path: paths.data_dir().join("skills"),
                scope: SkillScope::User,
                is_tool_root: false,
                is_plugin_root: false,
            });
            roots.push(SkillRoot {
                path: paths.legacy_dir().join("skills"),
                scope: SkillScope::User,
                is_tool_root: false,
                is_plugin_root: false,
            });
            roots.push(SkillRoot {
                path: paths.plugins_dir(),
                scope: SkillScope::User,
                is_tool_root: false,
                is_plugin_root: true,
            });
            roots.push(SkillRoot {
                path: paths.legacy_dir().join("plugins"),
                scope: SkillScope::User,
                is_tool_root: false,
                is_plugin_root: true,
            });
        }
    }

    #[cfg(unix)]
    roots.push(SkillRoot {
        path: PathBuf::from("/etc/codex/skills"),
        scope: SkillScope::Admin,
        is_tool_root: false,
        is_plugin_root: false,
    });

    if config.include_bundled_system_skills {
        roots.push(SkillRoot {
            path: system_cache_root_dir(&config.codex_home),
            scope: SkillScope::System,
            is_tool_root: false,
            is_plugin_root: false,
        });
    }

    roots
}

fn repo_skill_search_dirs(config: &SkillLoaderConfig) -> Vec<PathBuf> {
    let stop = config.project_root.clone().unwrap_or_else(|| config.cwd.clone());
    let mut dirs = Vec::new();
    let mut current = config.cwd.clone();

    loop {
        dirs.push(current.clone());
        if current == stop {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }

    dirs
}

pub(crate) fn find_git_root(path: &Path) -> Option<PathBuf> {
    let mut current = Some(path);
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

pub(crate) fn discover_skills_under_root(root: &SkillRoot, outcome: &mut SkillLoadOutcome) {
    walk_skills_dir(root, outcome, WalkMode::Full)
}

pub(crate) fn discover_metadata_under_root(root: &SkillRoot, outcome: &mut SkillLoadOutcome) {
    walk_skills_dir(root, outcome, WalkMode::Metadata)
}

#[derive(Clone, Copy)]
enum WalkMode {
    Full,
    Metadata,
}

fn walk_skills_dir(root: &SkillRoot, outcome: &mut SkillLoadOutcome, mode: WalkMode) {
    let Ok(root_path) = dunce::canonicalize(&root.path) else {
        return;
    };

    if !root_path.is_dir() {
        return;
    }

    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::from([(root_path, 0)]);
    let mut dirs_visited: usize = 0;
    while let Some((dir, depth)) = queue.pop_front() {
        dirs_visited += 1;
        if dirs_visited > MAX_DISCOVERY_DIRS {
            warn!(
                "skill discovery dir budget ({MAX_DISCOVERY_DIRS}) exhausted under {}; some skills may be missing",
                root.path.display()
            );
            return;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                if matches!(mode, WalkMode::Full) {
                    error!("failed to read skills dir {}: {e:#}", dir.display());
                } else {
                    tracing::debug!("failed to read skills dir {}: {e:#}", dir.display());
                }
                continue;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = match path.file_name().and_then(|f| f.to_str()) {
                Some(name) => name,
                None => continue,
            };

            if file_name.starts_with('.') || discovery_dir_skipped(file_name) {
                continue;
            }

            if path.is_dir() {
                // Plugin-consumed directories are units: a successful plugin
                // load ends handling here so the walk never descends into
                // plugin subdirectories looking for more skills.
                if root.is_tool_root
                    && let Ok(Some(tool_meta)) = try_load_tool_from_dir(&path, root.scope)
                {
                    outcome.skills.push(tool_meta);
                }

                if root.is_plugin_root
                    && let Ok(Some(plugin_meta)) = try_load_plugin_from_dir(&path, root.scope)
                {
                    outcome.skills.push(plugin_meta);
                    continue;
                }

                if try_ingest_plugin_skills(&path, root, outcome) {
                    continue;
                }

                if depth < MAX_DISCOVERY_DEPTH {
                    queue.push_back((path.clone(), depth + 1));
                }
            }

            if file_name == "SKILL.md" {
                handle_skill_md(path.clone(), root, outcome, mode);
            } else if matches!(mode, WalkMode::Full) && root.is_tool_root && is_executable_file(&path) {
                // Standalone executable tools are recognized in the full walk
                // but ignored in the lightweight metadata walk.
            }
        }
    }
}

fn handle_skill_md(path: PathBuf, root: &SkillRoot, outcome: &mut SkillLoadOutcome, mode: WalkMode) {
    let skill_dir = match path.parent() {
        Some(dir) => dir,
        None => return,
    };

    match mode {
        WalkMode::Full => match crate::skills::manifest::parse_skill_file(skill_dir) {
            Ok((manifest, _)) => {
                outcome.skills.push(SkillMetadata {
                    name: manifest.name.clone(),
                    description: manifest.description.clone(),
                    short_description: None,
                    path,
                    scope: root.scope,
                    manifest: Some(manifest.into()),
                });
            }
            Err(err) => {
                if root.scope != SkillScope::System {
                    outcome.errors.push(SkillErrorInfo { path, message: err.to_string() });
                }
            }
        },
        WalkMode::Metadata => match fs::read_to_string(&path).with_context(|| format!("reading {}", path.display())) {
            Ok(contents) => match crate::skills::manifest::parse_skill_content(&contents) {
                Ok((manifest, _)) => {
                    outcome.skills.push(SkillMetadata {
                        name: manifest.name.clone(),
                        description: manifest.description.clone(),
                        short_description: None,
                        path,
                        scope: root.scope,
                        manifest: Some(manifest.into()),
                    });
                }
                Err(err) => {
                    if root.scope != SkillScope::System {
                        outcome.errors.push(SkillErrorInfo { path, message: err.to_string() });
                    }
                }
            },
            Err(err) => {
                if root.scope != SkillScope::System {
                    outcome.errors.push(SkillErrorInfo { path, message: err.to_string() });
                }
            }
        },
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

/// Try to load a portable Agent Plugin from `path`. Returns `true` if a valid
/// plugin was loaded and its skills were ingested into `outcome`.
fn try_ingest_plugin_skills(path: &Path, root: &SkillRoot, outcome: &mut SkillLoadOutcome) -> bool {
    if !root.is_plugin_root {
        return false;
    }
    let Ok(loaded) = LoadedPlugin::load_from_dir(path) else {
        return false;
    };
    for skill in &loaded.skills {
        outcome.skills.push(SkillMetadata {
            name: skill.name.clone(),
            description: skill.manifest.description.clone(),
            short_description: None,
            path: skill.skill_md_path.clone(),
            scope: root.scope,
            manifest: Some(Box::new(skill.manifest.clone())),
        });
    }
    true
}

pub(crate) fn tool_config_to_metadata(config: &CliToolConfig, scope: SkillScope) -> Result<SkillMetadata> {
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

fn try_load_plugin_from_dir(path: &Path, scope: SkillScope) -> Result<Option<SkillMetadata>> {
    // Check if it's a native plugin directory (has plugin.json)
    let plugin_json_path = path.join("plugin.json");
    if !plugin_json_path.exists() {
        return Ok(None);
    }

    // Read and parse plugin metadata
    let plugin_json_content = fs::read_to_string(&plugin_json_path).context("Failed to read plugin.json")?;

    let plugin_metadata: crate::skills::native_plugin::PluginMetadata =
        serde_json::from_str(&plugin_json_content).context("Invalid plugin.json format")?;

    // Validate that the plugin has a corresponding dynamic library
    let lib_name = crate::skills::native_plugin::PluginLoader::new().library_filename(&plugin_metadata.name);

    if !path.join(&lib_name).exists() {
        // Try alternative library names
        let alternatives = [
            format!("lib{}.dylib", plugin_metadata.name),
            format!("{}.dylib", plugin_metadata.name),
            format!("lib{}.so", plugin_metadata.name),
            format!("{}.so", plugin_metadata.name),
            format!("{}.dll", plugin_metadata.name),
        ];

        let has_lib = alternatives.iter().any(|alt| path.join(alt).exists());
        if !has_lib {
            return Ok(None); // No library found, skip this plugin
        }
    }

    Ok(Some(SkillMetadata {
        name: plugin_metadata.name.clone(),
        description: plugin_metadata.description.clone(),
        short_description: None,
        path: path.to_path_buf(),
        scope,
        manifest: None, // Native plugins don't have SKILL.md manifest
    }))
}
