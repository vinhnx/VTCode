//! Skill Location Management
//!
//! Implements skill discovery across multiple locations with proper precedence,
//! following the pi-mono pattern for compatibility with Claude Code and Codex CLI.

use crate::skills::manifest::parse_skill_file;
use crate::skills::types::SkillContext;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Skill location types with precedence ordering
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillLocationType {
    /// VT Code user skills (highest precedence)
    VtcodeUser = 6,
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

impl SkillLocationType {
    /// Get location type from path
    #[allow(dead_code)]
    fn from_path(path: &Path) -> Option<Self> {
        let path_str = path.to_string_lossy();

        if path_str.contains(".vtcode/skills")
            && (path_str.contains("~/")
                || path_str.contains("/home/")
                || path_str.contains("/Users/"))
        {
            Some(SkillLocationType::VtcodeUser)
        } else if path_str.contains(".vtcode/skills") {
            Some(SkillLocationType::VtcodeProject)
        } else if path_str.contains(".pi/skills")
            && (path_str.contains("~/")
                || path_str.contains("/home/")
                || path_str.contains("/Users/"))
        {
            Some(SkillLocationType::PiUser)
        } else if path_str.contains(".pi/skills") {
            Some(SkillLocationType::PiProject)
        } else if path_str.contains(".claude/skills")
            && (path_str.contains("~/")
                || path_str.contains("/home/")
                || path_str.contains("/Users/"))
        {
            Some(SkillLocationType::ClaudeUser)
        } else if path_str.contains(".claude/skills") {
            Some(SkillLocationType::ClaudeProject)
        } else if path_str.contains(".codex/skills") {
            Some(SkillLocationType::CodexUser)
        } else {
            None
        }
    }
}

/// Skill location configuration
#[derive(Debug, Clone)]
pub struct SkillLocation {
    /// Location type for precedence
    pub location_type: SkillLocationType,

    /// Base directory path
    pub base_path: PathBuf,

    /// Scanning mode (recursive vs one-level)
    pub recursive: bool,

    /// Skill name separator for recursive mode
    pub name_separator: char,
}

impl SkillLocation {
    /// Create new skill location
    pub fn new(location_type: SkillLocationType, base_path: PathBuf, recursive: bool) -> Self {
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

    /// Check if this location exists
    pub fn exists(&self) -> bool {
        self.base_path.exists() && self.base_path.is_dir()
    }

    /// Get skill name from path
    pub fn get_skill_name(&self, skill_path: &Path) -> Option<String> {
        if !skill_path.exists() || !skill_path.is_dir() {
            return None;
        }

        // Check if this path contains a SKILL.md file
        let skill_md = skill_path.join("SKILL.md");
        if !skill_md.exists() {
            return None;
        }

        if self.recursive {
            // For recursive locations, build name with separators
            match skill_path.strip_prefix(&self.base_path) {
                Ok(relative_path) => {
                    let name_components: Vec<&str> = relative_path
                        .components()
                        .filter_map(|c| c.as_os_str().to_str())
                        .collect();

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
            skill_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|s| s.to_string())
        }
    }
}

/// Skill locations manager
pub struct SkillLocations {
    locations: Vec<SkillLocation>,
}

impl SkillLocations {
    /// Create new skill locations manager with default locations
    pub fn new() -> Self {
        Self::with_locations(Self::default_locations())
    }

    /// Create with custom locations
    pub fn with_locations(locations: Vec<SkillLocation>) -> Self {
        // Sort by precedence (highest first)
        let mut sorted_locations = locations;
        sorted_locations.sort_by_key(|loc| std::cmp::Reverse(loc.location_type));

        Self {
            locations: sorted_locations,
        }
    }

    /// Get default skill locations following pi-mono pattern
    pub fn default_locations() -> Vec<SkillLocation> {
        vec![
            // VT Code locations (highest precedence)
            SkillLocation::new(
                SkillLocationType::VtcodeUser,
                PathBuf::from("~/.vtcode/skills"),
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
                false, // one-level
            ),
            SkillLocation::new(
                SkillLocationType::ClaudeProject,
                PathBuf::from(".claude/skills"),
                false, // one-level
            ),
            // Codex CLI locations (recursive)
            SkillLocation::new(
                SkillLocationType::CodexUser,
                PathBuf::from("~/.codex/skills"),
                true, // recursive
            ),
        ]
    }

    /// Discover all skills across all locations
    pub fn discover_skills(&self) -> Result<Vec<DiscoveredSkill>> {
        let mut discovered_skills = HashMap::new(); // skill_name -> (location_type, skill_context)
        let mut discovery_stats = DiscoveryStats::default();

        info!(
            "Discovering skills across {} locations",
            self.locations.len()
        );

        for location in &self.locations {
            if !location.exists() {
                debug!("Location does not exist: {}", location.base_path.display());
                continue;
            }

            info!(
                "Scanning location: {} ({})",
                location.base_path.display(),
                if location.recursive {
                    "recursive"
                } else {
                    "one-level"
                }
            );

            discovery_stats.locations_scanned += 1;

            if location.recursive {
                self.scan_recursive_location(
                    location,
                    &mut discovered_skills,
                    &mut discovery_stats,
                )?;
            } else {
                self.scan_one_level_location(
                    location,
                    &mut discovered_skills,
                    &mut discovery_stats,
                )?;
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
            std::cmp::Ordering::Equal => a
                .skill_context
                .manifest()
                .name
                .cmp(&b.skill_context.manifest().name),
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
    if depth > 10 {
        // Prevent infinite recursion
        return Ok(());
    }

    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }

    // Check if this directory is a skill
    if let Some(skill_name) = location.get_skill_name(dir) {
        match parse_skill_file(dir) {
            Ok((manifest, _)) => {
                // Check if we already have this skill from a higher precedence location
                let had_existing = discovered.contains_key(&manifest.name);

                if let Some(existing) = discovered
                    .get(&manifest.name)
                    .filter(|e| e.location_type < location.location_type)
                {
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
                    skill_context: SkillContext::MetadataOnly(manifest.clone()),
                    skill_path: dir.to_path_buf(),
                    skill_name: skill_name.clone(),
                };

                discovered.insert(manifest.name.clone(), discovered_skill);
                stats.skills_found += 1;
                info!(
                    "Discovered skill: '{}' from {} at {}",
                    manifest.name,
                    location.location_type,
                    dir.display()
                );

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

    // Continue walking subdirectories
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_directory(&path, location, discovered, stats, depth + 1)?;
            }
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
        if !location.base_path.exists() || !location.base_path.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&location.base_path)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(skill_name) = location.get_skill_name(&path).filter(|_| path.is_dir()) {
                match parse_skill_file(&path) {
                    Ok((manifest, _)) => {
                        // Check precedence
                        if let Some(_existing) = discovered
                            .get(&manifest.name)
                            .filter(|e| e.location_type < location.location_type)
                        {
                            stats.skips_due_to_precedence += 1;
                            continue;
                        }

                        let discovered_skill = DiscoveredSkill {
                            location_type: location.location_type,
                            skill_context: SkillContext::MetadataOnly(manifest.clone()),
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
        self.locations
            .iter()
            .find(|loc| loc.location_type == location_type)
    }
}

/// Discovered skill with location information
#[derive(Debug, Clone)]
pub struct DiscoveredSkill {
    /// Location type where skill was found
    pub location_type: SkillLocationType,

    /// Skill context (metadata only)
    pub skill_context: SkillContext,

    /// Path to skill directory
    pub skill_path: PathBuf,

    /// Generated skill name (with separators for recursive)
    pub skill_name: String,
}

/// Discovery statistics
#[derive(Debug, Default)]
pub struct DiscoveryStats {
    pub locations_scanned: usize,
    pub skills_found: usize,
    pub skips_due_to_precedence: usize,
    pub skills_with_higher_precedence: usize,
    pub parse_errors: usize,
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
    use tempfile::TempDir;

    #[test]
    fn test_skill_location_type_precedence() {
        assert!(SkillLocationType::VtcodeUser > SkillLocationType::VtcodeProject);
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
    fn test_one_level_location() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create one-level skill structure
        let skill_path = base_path.join("file-analyzer");
        std::fs::create_dir_all(&skill_path).unwrap();
        std::fs::write(
            skill_path.join("SKILL.md"),
            "---\nname: file-analyzer\n---\n",
        )
        .unwrap();

        let location = SkillLocation::new(
            SkillLocationType::ClaudeProject,
            base_path.to_path_buf(),
            false, // one-level
        );

        let skill_name = location.get_skill_name(&skill_path);
        assert_eq!(skill_name, Some("file-analyzer".to_string()));
    }

    #[tokio::test]
    async fn test_location_discovery() {
        // Test with custom locations that exist in the current workspace
        // Use the workspace root (where .vtcode/skills actually is)
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let vtcode_skills = workspace_root.join(".vtcode/skills");
        let claude_skills = workspace_root.join(".claude/skills");

        println!("Testing with workspace root: {}", workspace_root.display());
        println!("VT Code skills path: {}", vtcode_skills.display());
        println!("Claude skills path: {}", claude_skills.display());
        println!("VT Code path exists: {}", vtcode_skills.exists());
        println!("Claude path exists: {}", claude_skills.exists());

        let locations = SkillLocations::with_locations(vec![
            SkillLocation::new(SkillLocationType::VtcodeProject, vtcode_skills, true),
            SkillLocation::new(SkillLocationType::ClaudeProject, claude_skills, false),
        ]);

        let discovered = locations.discover_skills().unwrap();

        println!("Discovered {} skills from test locations", discovered.len());

        // Should find at least the skills we moved to .vtcode/skills
        let skill_names: Vec<String> = discovered
            .iter()
            .map(|d| d.skill_context.manifest().name.clone())
            .collect();

        println!("Found skills: {:?}", skill_names);

        // Check for some of the skills we know should exist
        assert!(
            skill_names.contains(&"spreadsheet-generator".to_string())
                || skill_names.contains(&"doc-generator".to_string())
                || skill_names.contains(&"pdf-report-generator".to_string()),
            "Should have found at least one of the VT Code skills"
        );
    }

    #[test]
    fn test_full_integration() {
        println!("Testing full VT Code skills location system integration...");

        // Test with the actual workspace
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();

        // Test default locations
        let locations = SkillLocations::default();
        let discovered = locations.discover_skills().unwrap();

        println!("Default locations discovered {} skills", discovered.len());

        // Test specific VT Code location
        let vtcode_skills = workspace_root.join(".vtcode/skills");
        let vtcode_locations = SkillLocations::with_locations(vec![SkillLocation::new(
            SkillLocationType::VtcodeProject,
            vtcode_skills,
            true,
        )]);

        let vtcode_discovered = vtcode_locations.discover_skills().unwrap();
        println!(
            "VT Code location discovered {} skills",
            vtcode_discovered.len()
        );

        // Verify we found the expected skills
        let skill_names: Vec<String> = vtcode_discovered
            .iter()
            .map(|d| d.skill_context.manifest().name.clone())
            .collect();

        assert!(
            skill_names.contains(&"doc-generator".to_string()),
            "Should find doc-generator"
        );
        assert!(
            skill_names.contains(&"spreadsheet-generator".to_string()),
            "Should find spreadsheet-generator"
        );
        assert!(
            skill_names.contains(&"pdf-report-generator".to_string()),
            "Should find pdf-report-generator"
        );

        println!("Integration test passed! Found skills: {:?}", skill_names);
    }
}
