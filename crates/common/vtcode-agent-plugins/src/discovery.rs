use std::path::{Path, PathBuf};

use crate::errors::PluginError;
use crate::manifest::PluginManifest;
use crate::mcp::McpConfig;
use vtcode_skills::types::SkillManifest;

#[derive(Debug, Clone)]
pub struct DiscoveredSkill {
    pub name: String,
    pub dir_name: String,
    pub skill_md_path: PathBuf,
    pub manifest: SkillManifest,
}

#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub root: PathBuf,
    pub skills: Vec<DiscoveredSkill>,
    pub mcp: Option<McpConfig>,
}

impl LoadedPlugin {
    /// Load a plugin from a directory. This is the orchestrator: it calls the
    /// individual phases (manifest, skills, MCP) and wires them together.
    pub fn load_from_dir(plugin_root: &Path) -> Result<Self, PluginError> {
        // Filesystem-resolved root per §4.1: resolve symlinks so later
        // containment checks compare canonical paths.
        let absolute = std::path::absolute(plugin_root).map_err(PluginError::Io)?;
        let root = vtcode_commons::canonicalize(&absolute).unwrap_or(absolute);
        let manifest = Self::parse_manifest_dir(&root)?;
        let skills = Self::discover_skills_dir(&root)?;
        let mcp = Self::discover_mcp_dir(&root)?;

        // If MCP is present but its schema version disagrees with the manifest,
        // disable MCP for this plugin rather than failing the whole load.
        let mcp = match mcp {
            Some(ref mcp_config)
                if !mcp_config.schema.is_empty()
                    && !manifest.schema.is_empty()
                    && extract_schema_version(&manifest.schema) != extract_schema_version(&mcp_config.schema) =>
            {
                tracing::warn!(
                    manifest_version = ?extract_schema_version(&manifest.schema),
                    mcp_version = ?extract_schema_version(&mcp_config.schema),
                    "mcp.json schema version does not match plugin.json; disabling MCP for plugin"
                );
                None
            }
            other => other,
        };

        Ok(LoadedPlugin { manifest, root, skills, mcp })
    }

    /// Parse the `plugin.json` manifest from a plugin directory.
    pub fn parse_manifest_dir(plugin_root: &Path) -> Result<PluginManifest, PluginError> {
        let manifest_path = plugin_root.join("plugin.json");
        // §4.1 boundary 1: plugin.json escaping the root rejects the plugin.
        let _ = ensure_package_path_within(plugin_root, &manifest_path)?;
        let content = std::fs::read_to_string(&manifest_path)?;
        let (manifest, unknown_fields) = PluginManifest::parse(&content)?;
        for field in &unknown_fields {
            tracing::warn!(field = field, "unknown top-level field in plugin.json");
        }
        Ok(manifest)
    }

    /// Discover bundled skills under `skills/*/SKILL.md`.
    pub fn discover_skills_dir(plugin_root: &Path) -> Result<Vec<DiscoveredSkill>, PluginError> {
        discover_skills(plugin_root)
    }

    /// Discover the optional `mcp.json` MCP server configuration.
    pub fn discover_mcp_dir(plugin_root: &Path) -> Result<Option<McpConfig>, PluginError> {
        discover_mcp(plugin_root)
    }
}

fn extract_schema_version(schema: &str) -> Option<&str> {
    schema
        .strip_prefix("https://agent-plugins.org/schemas/")
        .and_then(|rest| rest.split('/').next())
}

fn discover_skills(plugin_root: &Path) -> Result<Vec<DiscoveredSkill>, PluginError> {
    let skills_dir = plugin_root.join("skills");
    // Missing location is valid absence (§6.2). Use symlink_metadata so a
    // missing path does not follow links.
    match std::fs::symlink_metadata(&skills_dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(PluginError::Io(e)),
        Ok(_) => {}
    }
    // §4.1 boundary 2 + §6.2: wrong kind or escape invalidates only this
    // component type; other types continue loading.
    if ensure_package_path_within(plugin_root, &skills_dir).is_err() {
        tracing::warn!(
            plugin_root = %plugin_root.display(),
            "skills location escapes plugin root; disabling skills for plugin"
        );
        return Ok(Vec::new());
    }
    if !skills_dir.is_dir() {
        tracing::warn!(
            plugin_root = %plugin_root.display(),
            "skills location is not a directory; disabling skills for plugin"
        );
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    for entry in std::fs::read_dir(&skills_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // Immediate child escaping via symlink is skipped, not fatal.
        if ensure_package_path_within(plugin_root, &path).is_err() {
            tracing::warn!(
                plugin_root = %plugin_root.display(),
                skill_dir = %path.display(),
                "skill directory escapes plugin root; skipping"
            );
            continue;
        }
        let skill_md = path.join("SKILL.md");
        // §4.1 boundary 3: SKILL.md escaping the root skips only that skill.
        if ensure_package_path_within(plugin_root, &skill_md).is_err() {
            tracing::warn!(
                plugin_root = %plugin_root.display(),
                skill_md = %skill_md.display(),
                "SKILL.md escapes plugin root; skipping skill"
            );
            continue;
        }
        if !skill_md.is_file() {
            continue;
        }

        let dir_name = path
            .file_name()
            .and_then(|s| s.to_string_lossy().into_owned().into())
            .unwrap_or_default();

        match load_one_skill(&skill_md, &dir_name) {
            Ok(skill) => {
                // Post mismatch-downgrade two dirs can share one manifest name;
                // first wins so the catalog stays deterministic.
                if skills.iter().any(|s: &DiscoveredSkill| s.name == skill.name) {
                    tracing::warn!(
                        plugin_root = %plugin_root.display(),
                        skill_name = %skill.name,
                        skill_dir = %dir_name,
                        "duplicate skill name in plugin; keeping first occurrence"
                    );
                    continue;
                }
                skills.push(skill);
            }
            // A broken skill must not take down the whole plugin: skip it and
            // continue with the remaining skills.
            Err(e) => {
                tracing::warn!(
                    plugin_root = %plugin_root.display(),
                    skill_dir = %dir_name,
                    error = %e,
                    "skipping invalid plugin skill"
                );
            }
        }
    }

    Ok(skills)
}

fn load_one_skill(skill_md: &Path, dir_name: &str) -> Result<DiscoveredSkill, PluginError> {
    let content = std::fs::read_to_string(skill_md)?;
    let (skill_manifest, _instructions) =
        vtcode_skills::manifest::parse_skill_content(&content).map_err(|e| PluginError::InvalidSkill(e.to_string()))?;

    // Agent Skills client guide + vtcode-skills::parse_skill_file: directory
    // mismatch warns but loads so cross-client renames still work. Discovery
    // keys by manifest name; dir_name is retained for diagnostics.
    if skill_manifest.name != dir_name {
        tracing::warn!(
            skill_name = %skill_manifest.name,
            dir_name = %dir_name,
            "skill name does not match directory; loading by manifest name"
        );
    }

    skill_manifest
        .validate()
        .map_err(|e| PluginError::InvalidSkill(e.to_string()))?;

    Ok(DiscoveredSkill {
        name: skill_manifest.name.clone(),
        dir_name: dir_name.to_string(),
        skill_md_path: skill_md.to_path_buf(),
        manifest: skill_manifest,
    })
}

fn ensure_package_path_within(plugin_root: &Path, candidate: &Path) -> Result<PathBuf, PluginError> {
    let canonical_root = vtcode_commons::canonicalize(plugin_root)
        .map_err(|e| PluginError::PathEscape(format!("plugin root could not be resolved: {e}")))?;
    // Resolve the candidate (following symlinks) or its deepest existing
    // ancestor when the leaf does not exist yet. Shared helper lives in
    // `expansion.rs` so the two call sites cannot drift.
    let resolved = crate::expansion::canonicalize_allow_missing(candidate)
        .map_err(|e| PluginError::PathEscape(format!("path could not be resolved: {} ({e})", candidate.display())))?;
    if resolved.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(PluginError::PathEscape(format!("path escapes plugin root: {}", candidate.display())));
    }
    if !resolved.starts_with(&canonical_root) {
        return Err(PluginError::PathEscape(format!("path escapes plugin root: {}", candidate.display())));
    }
    Ok(resolved)
}

fn discover_mcp(plugin_root: &Path) -> Result<Option<McpConfig>, PluginError> {
    let mcp_path = plugin_root.join("mcp.json");
    match std::fs::symlink_metadata(&mcp_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(PluginError::Io(e)),
        Ok(_) => {}
    }
    // §4.1 boundary 4 + §6.2: escape or wrong kind disables MCP only.
    if ensure_package_path_within(plugin_root, &mcp_path).is_err() {
        tracing::warn!(
            plugin_root = %plugin_root.display(),
            "mcp.json escapes plugin root; disabling MCP for plugin"
        );
        return Ok(None);
    }
    if !mcp_path.is_file() {
        tracing::warn!(
            plugin_root = %plugin_root.display(),
            "mcp.json is not a regular file; disabling MCP for plugin"
        );
        return Ok(None);
    }

    let content = std::fs::read_to_string(&mcp_path)?;
    let config = McpConfig::parse(&content)?;
    if !config.unknown_fields.is_empty() {
        tracing::warn!(
            fields = ?config.unknown_fields,
            "unknown top-level fields in plugin mcp.json; disabling MCP for this plugin"
        );
        return Ok(None);
    }
    Ok(Some(config))
}
