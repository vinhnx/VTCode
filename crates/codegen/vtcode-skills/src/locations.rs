//! Skill Location Management
//!
//! Implements skill discovery across multiple locations with proper precedence,
//! following the pi-mono pattern for compatibility with Claude Code and Codex CLI.

use crate::manifest::parse_skill_file;
use crate::types::SkillContext;
use anyhow::Result;
use hashbrown::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use vtcode_commons::VtCodePaths;
use vtcode_commons::walk::entry_is_dir;

/// Maximum recursion depth when walking a skill location.
///
/// The Agent Skills client guide recommends 4-6 levels; kept at 10 for backward
/// compatibility with deeply nested plugin layouts. Dependency and build-output
/// trees are excluded separately via [`SKIPPED_DISCOVERY_DIRS`].
pub const MAX_DISCOVERY_DEPTH: usize = 10;

/// Maximum directories visited per discovery walk.
///
/// Bounds runaway scans of huge trees (monorepos, home directories) per the
/// Agent Skills client guide's directory budget.
pub const MAX_DISCOVERY_DIRS: usize = 2000;

/// Directory names never descended into during skill discovery.
///
/// Build output, VCS metadata, and dependency trees cannot contain skills and
/// dominate scan time in large repos (notably `node_modules`).
pub const SKIPPED_DISCOVERY_DIRS: &[&str] = &[".git", ".hg", ".svn", "node_modules", "target"];

/// Whether a directory name is skipped during skill discovery.
pub fn discovery_dir_skipped(file_name: &str) -> bool {
    SKIPPED_DISCOVERY_DIRS.contains(&file_name)
}

/// Skill location types with precedence ordering
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillLocationType {
    /// VT Code user skills (highest precedence)
    VtcodeUser = 7,
    /// Project-level agent skills
    AgentsProject = 6,
    /// VT Code project skills
    VtcodeProject = 5,
    /// Pi user skills
    PiUser = 4,
    /// Pi project skills
    PiProject = 3,
    /// Claude Code user skills
    ClaudeUser = 2,
    /// Claude Code project skills
    ClaudeProject = 1,
    /// Codex CLI user skills (lowest precedence)
    CodexUser = 0,
}

impl SkillLocationType {}

/// Skill location configuration
#[derive(Debug, Clone)]
pub struct SkillLocation {
    /// Location type for precedence
    location_type: SkillLocationType,

    /// Base directory path
    base_path: PathBuf,

    /// Scanning mode (recursive vs one-level)
    recursive: bool,

    /// Skill name separator for recursive mode
    name_separator: char,
}

impl SkillLocation {
    /// Create new skill location
    fn new(location_type: SkillLocationType, base_path: PathBuf, recursive: bool) -> Self {
        let name_separator = match location_type {
            SkillLocationType::PiUser | SkillLocationType::PiProject => ':',
            _ => '/', // Default to path separator
        };

        Self {
            location_type,
            base_path,
            recursive,
            name_separator,
        }
    }

    /// Check if this location exists as a directory.
    ///
    /// `is_dir()` already resolves the path, so pairing it with `exists()`
    /// only added a second stat for the same answer.
    fn exists(&self) -> bool {
        self.base_path.is_dir()
    }

    /// Get the skill name for a directory that the caller already proved is one.
    ///
    /// Callers obtain that fact from `readdir` or a single `is_dir()`, so this
    /// deliberately does not re-resolve the path; only the `SKILL.md` probe is
    /// a real filesystem question.
    fn get_skill_name(&self, skill_path: &Path) -> Option<String> {
        // Check if this path contains a SKILL.md file
        let skill_md = skill_path.join("SKILL.md");
        if !skill_md.is_file() {
            return None;
        }

        if self.recursive {
            if matches!(self.location_type, SkillLocationType::ClaudeUser | SkillLocationType::ClaudeProject) {
                return skill_path.file_name().and_then(|name| name.to_str()).map(|s| s.to_string());
            }
            // For recursive locations, build name with separators
            match skill_path.strip_prefix(&self.base_path) {
                Ok(relative_path) => {
                    let name_components: Vec<&str> =
                        relative_path.components().filter_map(|c| c.as_os_str().to_str()).collect();

                    if name_components.is_empty() {
                        None
                    } else {
                        Some(name_components.join(&self.name_separator.to_string()))
                    }
                }
                Err(_) => None,
            }
        } else {
            // For one-level locations, just use the immediate directory name
            skill_path.file_name().and_then(|name| name.to_str()).map(|s| s.to_string())
        }
    }
}

/// Skill locations manager
pub struct SkillLocations {
    locations: Vec<SkillLocation>,
}

impl SkillLocations {
    /// Create new skill locations manager with default locations
    fn new() -> Self {
        Self::with_locations(Self::default_locations())
    }

    /// Create with custom locations
    fn with_locations(locations: Vec<SkillLocation>) -> Self {
        // Sort by precedence (highest first)
        let mut sorted_locations = locations;
        sorted_locations.sort_by_key(|loc| std::cmp::Reverse(loc.location_type));

        Self { locations: sorted_locations }
    }

    /// Get default skill locations following pi-mono pattern
    fn default_locations() -> Vec<SkillLocation> {
        let mut locations = Vec::new();
        if let Ok(paths) = VtCodePaths::resolve() {
            // VT Code locations (highest precedence). If the global path
            // policy is unavailable, omit this optional user location rather
            // than creating or scanning state relative to the cwd.
            locations.push(SkillLocation::new(
                SkillLocationType::VtcodeUser,
                paths.skills_dir(),
                true, // recursive
            ));
        }
        locations.extend([
            SkillLocation::new(
                SkillLocationType::AgentsProject,
                PathBuf::from(".agents/skills"),
                true, // recursive
            ),
            SkillLocation::new(
                SkillLocationType::VtcodeProject,
                PathBuf::from(".vtcode/skills"),
                true, // recursive
            ),
            // Pi locations (recursive with colon separator)
            SkillLocation::new(
                SkillLocationType::PiUser,
                PathBuf::from("~/.pi/agent/skills"),
                true, // recursive
            ),
            SkillLocation::new(
                SkillLocationType::PiProject,
                PathBuf::from(".pi/skills"),
                true, // recursive
            ),
            // Claude Code locations (one-level only)
            SkillLocation::new(
                SkillLocationType::ClaudeUser,
                PathBuf::from("~/.claude/skills"),
                true, // recursive
            ),
            SkillLocation::new(
                SkillLocationType::ClaudeProject,
                PathBuf::from(".claude/skills"),
                true, // recursive
            ),
            // Codex CLI locations (recursive)
            SkillLocation::new(
                SkillLocationType::CodexUser,
                PathBuf::from("~/.codex/skills"),
                true, // recursive
            ),
        ]);
        locations
    }

    /// Discover all skills across all locations
    fn discover_skills(&self) -> Result<Vec<DiscoveredSkill>> {
        let mut discovered_skills = HashMap::new(); // skill_name -> (location_type, skill_context)
        let mut discovery_stats = DiscoveryStats::default();

        info!("Discovering skills across {} locations", self.locations.len());

        for location in &self.locations {
            if !location.exists() {
                debug!("Location does not exist: {}", location.base_path.display());
                continue;
            }

            info!(
                "Scanning location: {} ({})",
                location.base_path.display(),
                if location.recursive { "recursive" } else { "one-level" }
            );

            discovery_stats.locations_scanned += 1;

            if location.recursive {
                self.scan_recursive_location(location, &mut discovered_skills, &mut discovery_stats)?;
            } else {
                self.scan_one_level_location(location, &mut discovered_skills, &mut discovery_stats)?;
            }
        }

        info!(
            "Discovery complete: {} skills found ({} from higher precedence locations)",
            discovered_skills.len(),
            discovery_stats.skills_with_higher_precedence
        );

        // Convert to final result
        let mut final_skills: Vec<DiscoveredSkill> = discovered_skills.into_values().collect();

        // Sort by location precedence (highest first) and then by name
        final_skills.sort_by(|a, b| match a.location_type.cmp(&b.location_type) {
            std::cmp::Ordering::Equal => a.skill_context.manifest().name.cmp(&b.skill_context.manifest().name),
            other => other.reverse(),
        });

        Ok(final_skills)
    }

    /// Scan recursive location (Pi/Codex style)
    fn scan_recursive_location(
        &self,
        location: &SkillLocation,
        discovered: &mut HashMap<String, DiscoveredSkill>,
        stats: &mut DiscoveryStats,
    ) -> Result<()> {
        walk_directory(&location.base_path, location, discovered, stats, 0)
    }
}

/// Walk directory recursively
fn walk_directory(
    dir: &Path,
    location: &SkillLocation,
    discovered: &mut HashMap<String, DiscoveredSkill>,
    stats: &mut DiscoveryStats,
    depth: usize,
) -> Result<()> {
    if depth > MAX_DISCOVERY_DEPTH {
        // Prevent infinite recursion
        return Ok(());
    }

    if !dir.is_dir() {
        return Ok(());
    }

    stats.dirs_visited += 1;
    if stats.dirs_visited > MAX_DISCOVERY_DIRS {
        debug!("skill discovery dir budget ({MAX_DISCOVERY_DIRS}) exhausted under {}", location.base_path.display());
        return Ok(());
    }

    // Check if this directory is a skill
    if let Some(skill_name) = location.get_skill_name(dir) {
        match parse_skill_file(dir) {
            Ok((manifest, _)) => {
                // Check if we already have this skill from a higher precedence location
                let existing_entry = discovered.get(&manifest.name);
                let had_existing = existing_entry.is_some();

                if let Some(existing) = existing_entry.filter(|e| e.location_type > location.location_type) {
                    // Existing skill has higher precedence, skip this one
                    stats.skips_due_to_precedence += 1;
                    debug!(
                        "Skipping skill '{}' from {} (already exists from higher precedence {})",
                        manifest.name, location.location_type, existing.location_type
                    );
                    return Ok(());
                }

                // Add or update the skill
                let discovered_skill = DiscoveredSkill {
                    location_type: location.location_type,
                    skill_context: SkillContext::MetadataOnly(manifest.clone(), dir.to_path_buf()),
                    skill_path: dir.to_path_buf(),
                    skill_name: skill_name.clone(),
                };

                discovered.insert(manifest.name.clone(), discovered_skill);
                stats.skills_found += 1;
                info!("Discovered skill: '{}' from {} at {}", manifest.name, location.location_type, dir.display());

                if had_existing {
                    stats.skills_with_higher_precedence += 1;
                }
            }
            Err(e) => {
                warn!("Failed to parse skill from {}: {}", dir.display(), e);
                stats.parse_errors += 1;
            }
        }
    }

    // Continue walking subdirectories, skipping dependency, build-output,
    // and VCS trees that cannot contain skills.
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            // Skip the name filter before touching the filesystem: a skipped
            // tree (`node_modules`, `target`) never needs its type resolved.
            if entry.file_name().to_str().is_some_and(discovery_dir_skipped) {
                continue;
            }
            if !entry_is_dir(&entry) {
                continue;
            }
            walk_directory(&entry.path(), location, discovered, stats, depth + 1)?;
        }
    }

    Ok(())
}

impl SkillLocations {
    /// Scan one-level location (Claude style)
    fn scan_one_level_location(
        &self,
        location: &SkillLocation,
        discovered: &mut HashMap<String, DiscoveredSkill>,
        stats: &mut DiscoveryStats,
    ) -> Result<()> {
        if !location.base_path.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&location.base_path)? {
            let entry = entry?;
            // The dirent type settles directory-ness without a second stat;
            // `get_skill_name` relies on this proof instead of re-resolving.
            if !entry_is_dir(&entry) {
                continue;
            }
            let path = entry.path();

            if let Some(skill_name) = location.get_skill_name(&path) {
                match parse_skill_file(&path) {
                    Ok((manifest, _)) => {
                        // Check precedence
                        if let Some(_existing) = discovered
                            .get(&manifest.name)
                            .filter(|e| e.location_type > location.location_type)
                        {
                            stats.skips_due_to_precedence += 1;
                            continue;
                        }

                        let discovered_skill = DiscoveredSkill {
                            location_type: location.location_type,
                            skill_context: SkillContext::MetadataOnly(manifest.clone(), path.clone()),
                            skill_path: path.clone(),
                            skill_name: skill_name.clone(),
                        };

                        discovered.insert(manifest.name.clone(), discovered_skill);
                        stats.skills_found += 1;
                        info!(
                            "Discovered skill: '{}' from {} at {}",
                            manifest.name,
                            location.location_type,
                            path.display()
                        );
                    }
                    Err(e) => {
                        warn!("Failed to parse skill from {}: {}", path.display(), e);
                        stats.parse_errors += 1;
                    }
                }
            }
        }

        Ok(())
    }

    /// Get all location types in precedence order
    pub fn get_location_types(&self) -> Vec<SkillLocationType> {
        self.locations.iter().map(|loc| loc.location_type).collect()
    }

    /// Get location by type
    pub fn get_location(&self, location_type: SkillLocationType) -> Option<&SkillLocation> {
        self.locations.iter().find(|loc| loc.location_type == location_type)
    }
}

/// Discovered skill with location information
#[derive(Debug, Clone)]
pub struct DiscoveredSkill {
    /// Location type where skill was found
    location_type: SkillLocationType,

    /// Skill context (metadata only)
    skill_context: SkillContext,

    /// Path to skill directory
    skill_path: PathBuf,

    /// Generated skill name (with separators for recursive)
    skill_name: String,
}

/// Discovery statistics
#[derive(Debug, Default)]
pub struct DiscoveryStats {
    locations_scanned: usize,
    skills_found: usize,
    skips_due_to_precedence: usize,
    skills_with_higher_precedence: usize,
    parse_errors: usize,
    dirs_visited: usize,
}

impl Default for SkillLocations {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert location type to string for display
impl std::fmt::Display for SkillLocationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillLocationType::VtcodeUser => write!(f, "VT Code User"),
            SkillLocationType::AgentsProject => write!(f, "Agents Project"),
            SkillLocationType::VtcodeProject => write!(f, "VT Code Project"),
            SkillLocationType::PiUser => write!(f, "Pi User"),
            SkillLocationType::PiProject => write!(f, "Pi Project"),
            SkillLocationType::ClaudeUser => write!(f, "Claude User"),
            SkillLocationType::ClaudeProject => write!(f, "Claude Project"),
            SkillLocationType::CodexUser => write!(f, "Codex User"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn create_test_skill(root: &Path, relative_dir: &str, name: &str) {
        let skill_dir = root.join(relative_dir);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let skill_md =
            format!("---\nname: {name}\ndescription: Test skill {name}\n---\n# {name}\n\nTest instructions.\n");
        std::fs::write(skill_dir.join("SKILL.md"), skill_md).unwrap();
    }

    #[test]
    fn test_discovery_dir_skip_list() {
        for skipped in [".git", ".hg", ".svn", "node_modules", "target"] {
            assert!(discovery_dir_skipped(skipped), "{skipped} must be skipped");
        }
        for scanned in ["my-skill", "skills", ".agents", "docs"] {
            assert!(!discovery_dir_skipped(scanned), "{scanned} must be scanned");
        }
    }

    #[test]
    fn test_walk_skips_dependency_and_vcs_trees() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        create_test_skill(base_path, "real-skill", "real-skill");
        // Skills nested under dependency/VCS/build trees must not be found.
        create_test_skill(base_path, "node_modules/hidden-skill", "hidden-skill");
        create_test_skill(base_path, ".git/hidden-git-skill", "hidden-git-skill");
        create_test_skill(base_path, "target/hidden-target-skill", "hidden-target-skill");

        let locations = SkillLocations::with_locations(vec![SkillLocation::new(
            SkillLocationType::VtcodeProject,
            base_path.to_path_buf(),
            true,
        )]);

        let discovered = locations.discover_skills().unwrap();
        let names: Vec<String> = discovered.iter().map(|d| d.skill_context.manifest().name.clone()).collect();
        assert_eq!(names, vec!["real-skill".to_string()]);
    }

    #[test]
    fn test_skill_location_type_precedence() {
        assert!(SkillLocationType::VtcodeUser > SkillLocationType::AgentsProject);
        assert!(SkillLocationType::AgentsProject > SkillLocationType::VtcodeProject);
        assert!(SkillLocationType::VtcodeProject > SkillLocationType::PiUser);
        assert!(SkillLocationType::PiUser > SkillLocationType::PiProject);
        assert!(SkillLocationType::PiProject > SkillLocationType::ClaudeUser);
        assert!(SkillLocationType::ClaudeUser > SkillLocationType::ClaudeProject);
        assert!(SkillLocationType::ClaudeProject > SkillLocationType::CodexUser);
    }

    #[test]
    fn test_skill_name_generation() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create nested skill structure
        let skill_path = base_path.join("web/tools/search-engine");
        std::fs::create_dir_all(&skill_path).unwrap();
        std::fs::write(skill_path.join("SKILL.md"), "---\nname: web-search\n---\n").unwrap();

        let location = SkillLocation::new(
            SkillLocationType::VtcodeProject,
            base_path.to_path_buf(),
            true, // recursive
        );

        let skill_name = location.get_skill_name(&skill_path);
        // VT Code uses '/' as separator for recursive locations
        assert_eq!(skill_name, Some("web/tools/search-engine".to_string()));
    }

    #[test]
    fn test_recursive_location() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create one-level skill structure
        let skill_path = base_path.join("file-analyzer");
        std::fs::create_dir_all(&skill_path).unwrap();
        std::fs::write(skill_path.join("SKILL.md"), "---\nname: file-analyzer\n---\n").unwrap();

        let location = SkillLocation::new(SkillLocationType::ClaudeProject, base_path.to_path_buf(), true);

        let skill_name = location.get_skill_name(&skill_path);
        assert_eq!(skill_name, Some("file-analyzer".to_string()));
    }

    #[tokio::test]
    async fn test_location_discovery() {
        let temp_dir = TempDir::new().unwrap();
        let project_skills = temp_dir.path().join(".agents/skills");
        let claude_skills = temp_dir.path().join(".claude/skills");

        create_test_skill(&project_skills, "docs/doc-generator", "doc-generator");
        create_test_skill(&project_skills, "spreadsheet-generator", "spreadsheet-generator");
        create_test_skill(&project_skills, "reports/pdf-report-generator", "pdf-report-generator");
        // Same manifest name in lower-precedence location should be ignored.
        create_test_skill(&claude_skills, "doc-generator", "doc-generator");

        let locations = SkillLocations::with_locations(vec![
            SkillLocation::new(SkillLocationType::AgentsProject, project_skills, true),
            SkillLocation::new(SkillLocationType::ClaudeProject, claude_skills, true),
        ]);

        let discovered = locations.discover_skills().unwrap();
        let skill_names: Vec<String> = discovered.iter().map(|d| d.skill_context.manifest().name.clone()).collect();
        assert!(skill_names.contains(&"doc-generator".to_string()));
        assert!(skill_names.contains(&"spreadsheet-generator".to_string()));
        assert!(skill_names.contains(&"pdf-report-generator".to_string()));

        let doc_generator = discovered
            .iter()
            .find(|d| d.skill_context.manifest().name == "doc-generator")
            .expect("doc-generator should be discovered");
        assert_eq!(doc_generator.location_type, SkillLocationType::AgentsProject);
    }

    #[test]
    fn test_full_integration() {
        let temp_dir = TempDir::new().unwrap();
        let agents_skills = temp_dir.path().join(".agents/skills");
        let vtcode_skills = temp_dir.path().join(".vtcode/skills");

        create_test_skill(&agents_skills, "doc-generator", "doc-generator");
        create_test_skill(&agents_skills, "spreadsheet-generator", "spreadsheet-generator");
        create_test_skill(&agents_skills, "pdf-report-generator", "pdf-report-generator");
        // Lower-precedence duplicate should be overwritten by AgentsProject entry.
        create_test_skill(&vtcode_skills, "doc-generator", "doc-generator");

        let locations = SkillLocations::with_locations(vec![
            SkillLocation::new(SkillLocationType::VtcodeProject, vtcode_skills, true),
            SkillLocation::new(SkillLocationType::AgentsProject, agents_skills, true),
        ]);

        let discovered = locations.discover_skills().unwrap();
        let skill_names: Vec<String> = discovered.iter().map(|d| d.skill_context.manifest().name.clone()).collect();

        assert!(skill_names.contains(&"doc-generator".to_string()), "Should find doc-generator");
        assert!(skill_names.contains(&"spreadsheet-generator".to_string()), "Should find spreadsheet-generator");
        assert!(skill_names.contains(&"pdf-report-generator".to_string()), "Should find pdf-report-generator");
        let doc_generator = discovered
            .iter()
            .find(|d| d.skill_context.manifest().name == "doc-generator")
            .expect("doc-generator should be discovered");
        assert_eq!(doc_generator.location_type, SkillLocationType::AgentsProject);
    }
}
