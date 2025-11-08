//! Tool versioning and compatibility management
//!
//! Implements semantic versioning for MCP tools with support for:
//! - Breaking change tracking
//! - Deprecation management
//! - Automatic skill migration
//! - Compatibility checking

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

/// Represents a specific version of a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolVersion {
    /// Tool name
    pub name: String,
    /// Semantic version components
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    /// When this version was released
    pub released: DateTime<Utc>,
    /// Human-readable description
    pub description: String,
    /// Input parameter schema
    pub input_schema: serde_json::Value,
    /// Output schema
    pub output_schema: serde_json::Value,
    /// Breaking changes from previous version
    pub breaking_changes: Vec<BreakingChange>,
    /// Deprecated fields
    pub deprecations: Vec<Deprecation>,
    /// Migration guide text
    pub migration_guide: Option<String>,
}

impl ToolVersion {
    pub fn version_string(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    /// Parse version string "1.2.3"
    pub fn from_string(s: &str) -> Result<(u32, u32, u32)> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(anyhow!("Invalid version format: {}", s));
        }
        Ok((parts[0].parse()?, parts[1].parse()?, parts[2].parse()?))
    }
}

/// Represents a breaking change in a tool version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakingChange {
    /// Field name that changed
    pub field: String,
    /// Old type/format
    pub old_type: String,
    /// New type/format
    pub new_type: String,
    /// Why this change was made
    pub reason: String,
    /// How to migrate code
    pub migration_code: String,
}

/// Represents a deprecated field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deprecation {
    /// Field name that's deprecated
    pub field: String,
    /// Replacement field if any
    pub replacement: Option<String>,
    /// Version in which it will be removed
    pub removed_in: String,
    /// Guidance for migration
    pub guidance: String,
}

/// Tool dependency in a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDependency {
    /// Tool name
    pub name: String,
    /// Required version (e.g., "1.2" for 1.2.x)
    pub version: String,
    /// Where this tool is used
    pub usage: Vec<String>,
}

/// Result of compatibility checking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityReport {
    pub compatible: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub migrations: Vec<Migration>,
}

/// A migration that needs to be applied
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Migration {
    pub skill_name: String,
    pub tool: String,
    pub from_version: String,
    pub to_version: String,
    pub transformations: Vec<CodeTransformation>,
}

/// A specific code transformation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeTransformation {
    pub line_number: usize,
    pub old_code: String,
    pub new_code: String,
    pub reason: String,
}

/// Checks tool version compatibility
pub enum VersionCompatibility {
    Compatible,
    Warning(String),
    RequiresMigration,
    Incompatible(String),
}

/// Checks if a skill is compatible with current tools
pub struct SkillCompatibilityChecker {
    skill_name: String,
    tool_dependencies: Vec<ToolDependency>,
    /// Current tool versions available
    tool_versions: HashMap<String, ToolVersion>,
}

impl SkillCompatibilityChecker {
    /// Create a new compatibility checker
    pub fn new(
        skill_name: String,
        tool_dependencies: Vec<ToolDependency>,
        tool_versions: HashMap<String, ToolVersion>,
    ) -> Self {
        Self {
            skill_name,
            tool_dependencies,
            tool_versions,
        }
    }

    /// Check if skill is compatible with current tools
    pub fn check_compatibility(&self) -> Result<CompatibilityReport> {
        let mut report = CompatibilityReport {
            compatible: true,
            warnings: vec![],
            errors: vec![],
            migrations: vec![],
        };

        for dep in &self.tool_dependencies {
            let current_tool = match self.tool_versions.get(&dep.name) {
                Some(v) => v,
                None => {
                    report.compatible = false;
                    report.errors.push(format!("Tool not found: {}", dep.name));
                    continue;
                }
            };

            match self.check_version_compatibility(&dep.version, &current_tool.version_string())? {
                VersionCompatibility::Compatible => {
                    debug!("Tool {} version {} is compatible", dep.name, dep.version);
                }
                VersionCompatibility::Warning(msg) => {
                    report.warnings.push(msg);
                    debug!(
                        "Compatibility warning for {}: {}",
                        dep.name,
                        report.warnings.last().unwrap()
                    );
                }
                VersionCompatibility::RequiresMigration => {
                    report.compatible = false;
                    // Would generate migration here
                    report.migrations.push(Migration {
                        skill_name: self.skill_name.clone(),
                        tool: dep.name.clone(),
                        from_version: dep.version.clone(),
                        to_version: current_tool.version_string(),
                        transformations: vec![],
                    });
                    debug!("Migration required for {} in {}", dep.name, self.skill_name);
                }
                VersionCompatibility::Incompatible(msg) => {
                    report.compatible = false;
                    report.errors.push(msg);
                    debug!(
                        "Incompatibility error for {}: {}",
                        dep.name,
                        report.errors.last().unwrap()
                    );
                }
            }
        }

        Ok(report)
    }

    /// Check semantic version compatibility
    fn check_version_compatibility(
        &self,
        required: &str,
        available: &str,
    ) -> Result<VersionCompatibility> {
        // required format: "1.2" (accepts 1.2.x)
        // available format: "1.2.3" (current version)

        let req_parts: Vec<&str> = required.split('.').collect();
        if req_parts.len() < 1 || req_parts.len() > 2 {
            return Err(anyhow!("Invalid required version format: {}", required));
        }

        let req_major: u32 = req_parts[0].parse()?;
        let req_minor: u32 = if req_parts.len() == 2 {
            req_parts[1].parse()?
        } else {
            0
        };

        let (avail_major, avail_minor, _avail_patch) = ToolVersion::from_string(available)?;

        let compat = match (req_major == avail_major, req_minor == avail_minor) {
            (true, true) => {
                // Major and minor match: compatible
                VersionCompatibility::Compatible
            }
            (true, false) if avail_minor > req_minor => {
                // Major matches, available minor is newer: warning
                VersionCompatibility::Warning(format!(
                    "Tool available version {} is newer than required {}",
                    available, required
                ))
            }
            (true, false) if avail_minor < req_minor => {
                // Major matches, available minor is older: requires migration
                VersionCompatibility::RequiresMigration
            }
            (false, _) if avail_major > req_major => {
                // Major version upgrade: usually breaking
                VersionCompatibility::Incompatible(format!(
                    "Tool major version changed from {} to {}",
                    req_major, avail_major
                ))
            }
            _ => {
                // Anything else is incompatible
                VersionCompatibility::Incompatible(format!(
                    "Tool version {} not compatible with required {}",
                    available, required
                ))
            }
        };

        Ok(compat)
    }

    /// Get detailed compatibility errors
    pub fn detailed_report(&self) -> Result<String> {
        let report = self.check_compatibility()?;

        let mut output = format!("Skill: {}\n", self.skill_name);
        output.push_str(&format!("Compatible: {}\n", report.compatible));

        if !report.warnings.is_empty() {
            output.push_str("\nWarnings:\n");
            for warning in &report.warnings {
                output.push_str(&format!("  - {}\n", warning));
            }
        }

        if !report.errors.is_empty() {
            output.push_str("\nErrors:\n");
            for error in &report.errors {
                output.push_str(&format!("  - {}\n", error));
            }
        }

        if !report.migrations.is_empty() {
            output.push_str("\nRequired Migrations:\n");
            for migration in &report.migrations {
                output.push_str(&format!(
                    "  - {}: {} -> {}\n",
                    migration.tool, migration.from_version, migration.to_version
                ));
            }
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tool(name: &str, version: &str) -> ToolVersion {
        let (major, minor, patch) = ToolVersion::from_string(version).unwrap();
        ToolVersion {
            name: name.to_string(),
            major,
            minor,
            patch,
            released: Utc::now(),
            description: format!("Test tool {}", version),
            input_schema: serde_json::json!({}),
            output_schema: serde_json::json!({}),
            breaking_changes: vec![],
            deprecations: vec![],
            migration_guide: None,
        }
    }

    #[test]
    fn test_version_parsing() {
        let (major, minor, patch) = ToolVersion::from_string("1.2.3").unwrap();
        assert_eq!(major, 1);
        assert_eq!(minor, 2);
        assert_eq!(patch, 3);

        // Invalid formats
        assert!(ToolVersion::from_string("1.2").is_err());
        assert!(ToolVersion::from_string("invalid").is_err());
    }

    #[test]
    fn test_exact_version_compatibility() {
        let mut tools = HashMap::new();
        tools.insert(
            "read_file".to_string(),
            create_test_tool("read_file", "1.2.3"),
        );

        let deps = vec![ToolDependency {
            name: "read_file".to_string(),
            version: "1.2".to_string(),
            usage: vec!["test".to_string()],
        }];

        let checker = SkillCompatibilityChecker::new("test_skill".to_string(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        assert!(report.compatible);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_missing_tool() {
        let tools = HashMap::new(); // No tools defined

        let deps = vec![ToolDependency {
            name: "nonexistent_tool".to_string(),
            version: "1.0".to_string(),
            usage: vec![],
        }];

        let checker = SkillCompatibilityChecker::new("test_skill".to_string(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        assert!(!report.compatible);
        assert!(!report.errors.is_empty());
    }

    #[test]
    fn test_minor_version_upgrade_warning() {
        let mut tools = HashMap::new();
        // Tool is at 1.3.0 but skill requires 1.2
        tools.insert(
            "list_files".to_string(),
            create_test_tool("list_files", "1.3.0"),
        );

        let deps = vec![ToolDependency {
            name: "list_files".to_string(),
            version: "1.2".to_string(),
            usage: vec![],
        }];

        let checker = SkillCompatibilityChecker::new("test_skill".to_string(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        assert!(report.compatible);
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn test_major_version_incompatibility() {
        let mut tools = HashMap::new();
        // Tool upgraded to 2.0.0, skill requires 1.2
        tools.insert(
            "grep_file".to_string(),
            create_test_tool("grep_file", "2.0.0"),
        );

        let deps = vec![ToolDependency {
            name: "grep_file".to_string(),
            version: "1.2".to_string(),
            usage: vec![],
        }];

        let checker = SkillCompatibilityChecker::new("test_skill".to_string(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        assert!(!report.compatible);
        assert!(!report.errors.is_empty());
    }

    #[test]
    fn test_detailed_report() {
        let mut tools = HashMap::new();
        tools.insert(
            "read_file".to_string(),
            create_test_tool("read_file", "1.2.3"),
        );

        let deps = vec![ToolDependency {
            name: "read_file".to_string(),
            version: "1.2".to_string(),
            usage: vec!["main".to_string()],
        }];

        let checker = SkillCompatibilityChecker::new("filter_skill".to_string(), deps, tools);
        let report = checker.detailed_report().unwrap();

        assert!(report.contains("filter_skill"));
        assert!(report.contains("Compatible: true"));
    }

    #[test]
    fn test_skill_compatible_with_newer_patch_version() {
        // Skill was written for list_files 1.2.0, but 1.2.5 is available
        // Should be compatible (patch version changes are backward compatible)
        let mut tools = HashMap::new();
        tools.insert(
            "list_files".to_string(),
            create_test_tool("list_files", "1.2.5"),
        );

        let deps = vec![ToolDependency {
            name: "list_files".to_string(),
            version: "1.2".to_string(),
            usage: vec!["main".to_string()],
        }];

        let checker = SkillCompatibilityChecker::new("filter_skill".to_string(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        assert!(
            report.compatible,
            "Should be compatible with patch version upgrade"
        );
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_multiple_tool_dependencies() {
        // Skill depends on multiple tools with different version compatibility
        let mut tools = HashMap::new();
        tools.insert(
            "read_file".to_string(),
            create_test_tool("read_file", "1.2.0"),
        );
        tools.insert(
            "write_file".to_string(),
            create_test_tool("write_file", "2.0.0"),
        );
        tools.insert(
            "list_files".to_string(),
            create_test_tool("list_files", "1.3.0"),
        );

        let deps = vec![
            ToolDependency {
                name: "read_file".to_string(),
                version: "1.2".to_string(),
                usage: vec!["read_input".to_string()],
            },
            ToolDependency {
                name: "write_file".to_string(),
                version: "1.0".to_string(),
                usage: vec!["write_output".to_string()],
            },
            ToolDependency {
                name: "list_files".to_string(),
                version: "1.2".to_string(),
                usage: vec!["scan_directory".to_string()],
            },
        ];

        let checker = SkillCompatibilityChecker::new("complex_skill".to_string(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        // Should be compatible for read_file and list_files, but need migration for write_file
        assert!(
            !report.compatible,
            "Should not be fully compatible due to write_file"
        );
        assert!(!report.errors.is_empty());
    }
}
