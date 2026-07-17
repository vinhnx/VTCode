//! Tool versioning and compatibility management
//!
//! Implements semantic versioning for MCP tools with support for:
//! - Breaking change tracking
//! - Deprecation management
//! - Automatic skill migration
//! - Compatibility checking

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use tracing::debug;
use vtcode_commons::MultiErrors;

#[cfg(test)]
use crate::config::constants::tools;

/// Represents a specific version of a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolVersion {
    /// Tool name
    pub name: String,
    /// Semantic version components
    pub major: u32,
    /// Minor version component.
    pub minor: u32,
    /// Patch version component.
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
    /// Format the version as `"major.minor.patch"`.
    pub fn version_string(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    /// Parse version string "1.2.3"
    pub fn from_string(s: &str) -> Result<(u32, u32, u32)> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(anyhow!("Invalid version format: {s}"));
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
    /// Whether the skill is fully compatible with available tool versions.
    pub compatible: bool,
    /// Non-fatal warnings (e.g., available version is newer than required).
    pub warnings: Vec<String>,
    /// Fatal errors preventing compatibility.
    pub errors: MultiErrors<String>,
    /// Migrations that must be applied to resolve incompatibilities.
    pub migrations: Vec<Migration>,
}

/// A migration that needs to be applied
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Migration {
    /// Name of the skill that needs migration.
    pub skill_name: String,
    /// Name of the tool whose version changed.
    pub tool: String,
    /// Version the skill was written against.
    pub from_version: String,
    /// Current available version of the tool.
    pub to_version: String,
    /// Code transformations to apply.
    pub transformations: Vec<CodeTransformation>,
}

/// A specific code transformation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeTransformation {
    /// 1-based line number in the skill source code.
    pub line_number: usize,
    /// Original code line.
    pub old_code: String,
    /// Replacement code line.
    pub new_code: String,
    /// Explanation of why this change is needed.
    pub reason: String,
}

/// Checks tool version compatibility
pub enum VersionCompatibility {
    /// Versions are fully compatible.
    Compatible,
    /// Compatible but with a non-fatal warning message.
    Warning(String),
    /// Requires a migration to become compatible.
    RequiresMigration,
    /// Not compatible; includes an error message.
    Incompatible(String),
}

/// Checks if a skill is compatible with current tools
pub struct SkillCompatibilityChecker {
    /// Name of the skill being checked.
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
        Self { skill_name, tool_dependencies, tool_versions }
    }

    /// Check if skill is compatible with current tools
    pub fn check_compatibility(&self) -> Result<CompatibilityReport> {
        let mut report = CompatibilityReport {
            compatible: true,
            warnings: vec![],
            errors: MultiErrors::new(),
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
                    report.warnings.push(msg.clone());
                    debug!("Compatibility warning for {}: {}", dep.name, msg);
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
                    report.errors.push(msg.clone());
                    debug!("Incompatibility error for {}: {}", dep.name, msg);
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
        if req_parts.is_empty() || req_parts.len() > 2 {
            return Err(anyhow!("Invalid required version format: {required}"));
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
                    "Tool available version {available} is newer than required {required}"
                ))
            }
            (true, false) if avail_minor < req_minor => {
                // Major matches, available minor is older: requires migration
                VersionCompatibility::RequiresMigration
            }
            (false, _) if avail_major > req_major => {
                // Major version upgrade: usually breaking
                VersionCompatibility::Incompatible(format!(
                    "Tool major version changed from {req_major} to {avail_major}"
                ))
            }
            _ => {
                // Anything else is incompatible
                VersionCompatibility::Incompatible(format!(
                    "Tool version {available} not compatible with required {required}"
                ))
            }
        };

        Ok(compat)
    }

    /// Get detailed compatibility errors
    pub fn detailed_report(&self) -> Result<String> {
        let report = self.check_compatibility()?;

        let mut output = format!("Skill: {}\n", self.skill_name);
        let _ = writeln!(output, "Compatible: {}", report.compatible);

        if !report.warnings.is_empty() {
            output.push_str("\nWarnings:\n");
            for warning in &report.warnings {
                let _ = writeln!(output, "  - {warning}");
            }
        }

        if !report.errors.is_empty() {
            output.push_str("\nErrors:\n");
            for error in report.errors.iter() {
                let _ = writeln!(output, "  - {error}");
            }
        }

        if !report.migrations.is_empty() {
            output.push_str("\nRequired Migrations:\n");
            for migration in &report.migrations {
                let _ = writeln!(
                    output,
                    "  - {}: {} -> {}",
                    migration.tool, migration.from_version, migration.to_version
                );
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
            name: name.to_owned(),
            major,
            minor,
            patch,
            released: Utc::now(),
            description: format!("Test tool {version}"),
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
        ToolVersion::from_string("1.2").unwrap_err();
        ToolVersion::from_string("invalid").unwrap_err();
    }

    #[test]
    fn test_exact_version_compatibility() {
        let mut tools = HashMap::new();
        tools.insert("read_file".to_owned(), create_test_tool("read_file", "1.2.3"));

        let deps = vec![ToolDependency {
            name: "read_file".to_owned(),
            version: "1.2".to_owned(),
            usage: vec!["test".to_owned()],
        }];

        let checker = SkillCompatibilityChecker::new("test_skill".to_owned(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        assert!(report.compatible);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_missing_tool() {
        let tools = HashMap::new(); // No tools defined

        let deps = vec![ToolDependency {
            name: "nonexistent_tool".to_owned(),
            version: "1.0".to_owned(),
            usage: vec![],
        }];

        let checker = SkillCompatibilityChecker::new("test_skill".to_owned(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        assert!(!report.compatible);
        assert!(!report.errors.is_empty());
    }

    #[test]
    fn test_minor_version_upgrade_warning() {
        let mut tools = HashMap::new();
        // Tool is at 1.3.0 but skill requires 1.2
        tools.insert(tools::LIST_FILES.to_owned(), create_test_tool(tools::LIST_FILES, "1.3.0"));

        let deps = vec![ToolDependency {
            name: tools::LIST_FILES.to_owned(),
            version: "1.2".to_owned(),
            usage: vec![],
        }];

        let checker = SkillCompatibilityChecker::new("test_skill".to_owned(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        assert!(report.compatible);
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn test_major_version_incompatibility() {
        let mut tools = HashMap::new();
        // Tool upgraded to 2.0.0, skill requires 1.2
        tools.insert(tools::GREP_FILE.to_owned(), create_test_tool(tools::GREP_FILE, "2.0.0"));

        let deps = vec![ToolDependency {
            name: tools::GREP_FILE.to_owned(),
            version: "1.2".to_owned(),
            usage: vec![],
        }];

        let checker = SkillCompatibilityChecker::new("test_skill".to_owned(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        assert!(!report.compatible);
        assert!(!report.errors.is_empty());
    }

    #[test]
    fn test_detailed_report() {
        let mut tools = HashMap::new();
        tools.insert("read_file".to_owned(), create_test_tool("read_file", "1.2.3"));

        let deps = vec![ToolDependency {
            name: "read_file".to_owned(),
            version: "1.2".to_owned(),
            usage: vec!["main".to_owned()],
        }];

        let checker = SkillCompatibilityChecker::new("filter_skill".to_owned(), deps, tools);
        let report = checker.detailed_report().unwrap();

        assert!(report.contains("filter_skill"));
        assert!(report.contains("Compatible: true"));
    }

    #[test]
    fn test_skill_compatible_with_newer_patch_version() {
        // Skill was written for list_files 1.2.0, but 1.2.5 is available
        // Should be compatible (patch version changes are backward compatible)
        let mut tools = HashMap::new();
        tools.insert(tools::LIST_FILES.to_owned(), create_test_tool(tools::LIST_FILES, "1.2.5"));

        let deps = vec![ToolDependency {
            name: tools::LIST_FILES.to_owned(),
            version: "1.2".to_owned(),
            usage: vec!["main".to_owned()],
        }];

        let checker = SkillCompatibilityChecker::new("filter_skill".to_owned(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        assert!(report.compatible, "Should be compatible with patch version upgrade");
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_multiple_tool_dependencies() {
        // Skill depends on multiple tools with different version compatibility
        let mut tools = HashMap::new();
        tools.insert("read_file".to_owned(), create_test_tool("read_file", "1.2.0"));
        tools.insert("write_file".to_owned(), create_test_tool("write_file", "2.0.0"));
        tools.insert(tools::LIST_FILES.to_owned(), create_test_tool(tools::LIST_FILES, "1.3.0"));

        let deps = vec![
            ToolDependency {
                name: "read_file".to_owned(),
                version: "1.2".to_owned(),
                usage: vec!["read_input".to_owned()],
            },
            ToolDependency {
                name: "write_file".to_owned(),
                version: "1.0".to_owned(),
                usage: vec!["write_output".to_owned()],
            },
            ToolDependency {
                name: tools::LIST_FILES.to_owned(),
                version: "1.2".to_owned(),
                usage: vec!["scan_directory".to_owned()],
            },
        ];

        let checker = SkillCompatibilityChecker::new("complex_skill".to_owned(), deps, tools);
        let report = checker.check_compatibility().unwrap();

        // Should be compatible for read_file and list_files, but need migration for write_file
        assert!(!report.compatible, "Should not be fully compatible due to write_file");
        assert!(!report.errors.is_empty());
    }
}
