//! Skill Validation System
//!
//! Validates skill definitions, configurations, and executions to ensure:
//! - Proper SKILL.md format and metadata
//! - Valid JSON schemas for tool arguments
//! - Executable scripts and tools
//! - Security and safety checks
//! - Performance and resource usage validation

use crate::skills::cli_bridge::CliToolConfig;
use crate::skills::manifest::parse_skill_file;
use crate::utils::file_utils::read_file_with_context_sync;
use anyhow::Result;
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};
use tracing::info;

mod checks;

/// Validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    /// Enable security checks
    pub enable_security_checks: bool,

    /// Enable performance validation
    pub enable_performance_checks: bool,

    /// Maximum execution time for validation tests (seconds)
    pub max_validation_time: u64,

    /// Maximum script size (bytes)
    pub max_script_size: usize,

    /// Allowed script extensions
    pub allowed_script_extensions: Vec<String>,

    /// Blocked commands/patterns
    pub blocked_commands: Vec<String>,

    /// Required metadata fields
    pub required_metadata_fields: Vec<String>,

    /// Enable JSON schema validation
    pub enable_schema_validation: bool,

    /// Strict mode (fail on warnings)
    pub strict_mode: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            enable_security_checks: true,
            enable_performance_checks: true,
            max_validation_time: 30,
            max_script_size: 1024 * 1024, // 1MB
            allowed_script_extensions: vec![
                "py".to_string(),
                "sh".to_string(),
                "bash".to_string(),
                "js".to_string(),
                "ts".to_string(),
                "rb".to_string(),
                "pl".to_string(),
                "go".to_string(),
                "rs".to_string(),
            ],
            blocked_commands: vec![
                "rm -rf /".to_string(),
                "sudo".to_string(),
                "chmod 777".to_string(),
                "curl.*|.*sh".to_string(),
                "wget.*|.*sh".to_string(),
            ],
            required_metadata_fields: vec!["name".to_string(), "description".to_string()],
            enable_schema_validation: true,
            strict_mode: false,
        }
    }
}

/// Validation result with detailed report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Overall validation status
    pub status: ValidationStatus,

    /// Skill name
    pub skill_name: String,

    /// Validation timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// Individual check results
    pub checks: HashMap<String, CheckResult>,

    /// Performance metrics
    pub performance: PerformanceMetrics,

    /// Security assessment
    pub security: SecurityAssessment,

    /// Recommendations for improvement
    pub recommendations: Vec<String>,
}

/// Validation status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ValidationStatus {
    /// All checks passed
    Valid,
    /// Some warnings, but skill is usable
    Warning,
    /// Critical issues, skill should not be used
    Invalid,
}

/// Individual check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// Check name
    pub name: String,

    /// Check status
    pub status: CheckStatus,

    /// Detailed message
    pub message: String,

    /// Additional details
    pub details: Option<Value>,

    /// Execution time
    pub execution_time_ms: u64,
}

/// Check status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CheckStatus {
    /// Check passed
    Passed,
    /// Check passed with warnings
    Warning,
    /// Check failed
    Failed,
    /// Check was skipped
    Skipped,
}

/// Performance metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Total validation time
    pub total_time_ms: u64,

    /// Skill loading time
    pub loading_time_ms: u64,

    /// Schema validation time
    pub schema_validation_time_ms: u64,

    /// Script validation time
    pub script_validation_time_ms: u64,

    /// Memory usage estimate (bytes)
    pub memory_usage_bytes: usize,

    /// Token usage estimate
    pub token_usage_estimate: usize,
}

/// Security assessment
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecurityAssessment {
    /// Overall security level
    pub security_level: SecurityLevel,

    /// Security warnings
    pub warnings: Vec<SecurityWarning>,

    /// Blocked content found
    pub blocked_content: Vec<String>,

    /// Safe to execute
    pub safe_to_execute: bool,
}

/// Security level
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum SecurityLevel {
    /// No security concerns
    #[default]
    Safe,
    /// Minor concerns, generally safe
    LowRisk,
    /// Moderate concerns, review recommended
    MediumRisk,
    /// High concerns, not recommended
    HighRisk,
}

/// Security warning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityWarning {
    /// Warning type
    pub warning_type: String,

    /// Warning message
    pub message: String,

    /// Severity level
    pub severity: SecurityLevel,

    /// Suggested remediation
    pub suggestion: Option<String>,
}

/// Skill validator
pub struct SkillValidator {
    config: ValidationConfig,
    // Note: Validator from jsonschema crate doesn't implement Clone,
    // so we cache the validation result keyed by path and mtime instead.
    schema_validation_cache: HashMap<PathBuf, (SystemTime, CheckResult)>,
}

impl SkillValidator {
    /// Create new validator with default configuration
    pub fn new() -> Self {
        Self::with_config(ValidationConfig::default())
    }
}

impl Default for SkillValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillValidator {
    /// Create new validator with custom configuration
    pub fn with_config(config: ValidationConfig) -> Self {
        Self { config, schema_validation_cache: HashMap::new() }
    }

    /// Validate a traditional skill from directory
    pub async fn validate_skill_directory(&mut self, skill_path: &Path) -> Result<ValidationReport> {
        let start_time = Instant::now();
        let mut checks = HashMap::new();
        // Performance tracking initialized at end

        info!("Validating skill directory: {}", skill_path.display());

        // Check if directory exists
        let check_result = self.check_directory_exists(skill_path).await;
        checks.insert("directory_exists".to_string(), check_result);

        // Validate SKILL.md file
        let skill_file = skill_path.join("SKILL.md");
        let check_result = self.validate_skill_file(&skill_file).await;
        checks.insert("skill_file_valid".to_string(), check_result.clone());

        let skill_name = if let Some(manifest) = &check_result.details {
            manifest.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string()
        } else {
            "unknown".to_string()
        };

        // Validate scripts directory
        let scripts_dir = skill_path.join("scripts");
        if scripts_dir.exists() {
            let check_result = self.validate_scripts_directory(&scripts_dir).await;
            checks.insert("scripts_valid".to_string(), check_result);
        }

        // Validate resources
        let resources_result = self.validate_resources(skill_path).await;
        for (name, result) in resources_result {
            checks.insert(format!("resource_{name}"), result);
        }

        // Security assessment
        let security = self.assess_security(&checks);

        // Generate recommendations
        let recommendations = self.generate_recommendations(&checks, &security);

        // Determine overall status
        let status = self.determine_overall_status(&checks, &security);

        let performance = PerformanceMetrics {
            total_time_ms: start_time.elapsed().as_millis() as u64,
            ..Default::default()
        };

        Ok(ValidationReport {
            status,
            skill_name,
            timestamp: chrono::Utc::now(),
            checks,
            performance,
            security,
            recommendations,
        })
    }

    /// Validate CLI tool configuration
    pub async fn validate_cli_tool(&mut self, config: &CliToolConfig) -> Result<ValidationReport> {
        let start_time = Instant::now();
        let mut checks = HashMap::new();

        info!("Validating CLI tool: {}", config.name);

        // Check executable exists
        let check_result = self.check_executable_exists(&config.executable_path).await;
        checks.insert("executable_exists".to_string(), check_result);

        // Check executable permissions
        let check_result = self.check_executable_permissions(&config.executable_path).await;
        checks.insert("executable_permissions".to_string(), check_result);

        // Validate README if present
        if let Some(readme_path) = &config.readme_path {
            let check_result = self.validate_readme_file(readme_path).await;
            checks.insert("readme_valid".to_string(), check_result);
        }

        // Validate JSON schema if present
        if let Some(schema_path) = &config.schema_path {
            let check_result = self.validate_json_schema(schema_path).await;
            checks.insert("schema_valid".to_string(), check_result);
        }

        // Test tool execution (basic)
        let check_result = self.test_tool_execution(config).await;
        checks.insert("tool_executable".to_string(), check_result);

        // Security assessment
        let security = self.assess_security(&checks);

        // Generate recommendations
        let recommendations = self.generate_recommendations(&checks, &security);

        // Determine overall status
        let status = self.determine_overall_status(&checks, &security);

        let performance = PerformanceMetrics {
            total_time_ms: start_time.elapsed().as_millis() as u64,
            ..Default::default()
        };

        Ok(ValidationReport {
            status,
            skill_name: config.name.clone(),
            timestamp: chrono::Utc::now(),
            checks,
            performance,
            security,
            recommendations,
        })
    }

    /// Validate JSON schema
    async fn validate_json_schema(&mut self, schema_path: &Path) -> CheckResult {
        let start_time = Instant::now();

        if !schema_path.exists() {
            return CheckResult {
                name: "schema_valid".to_string(),
                status: CheckStatus::Warning,
                message: "Schema file not found".to_string(),
                details: None,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
            };
        }

        // Check cache
        if let Ok(metadata) = tokio::fs::metadata(schema_path).await
            && let Ok(mtime) = metadata.modified()
            && let Some((cached_mtime, cached_result)) = self.schema_validation_cache.get(schema_path)
            && *cached_mtime == mtime
        {
            let mut result = cached_result.clone();
            // Update execution time to reflect cache hit (near zero)
            result.execution_time_ms = start_time.elapsed().as_millis() as u64;
            result.message = format!("{} (cached)", result.message);
            return result;
        }

        let result = match read_file_with_context_sync(schema_path, "skill JSON schema") {
            Ok(content) => {
                match serde_json::from_str::<Value>(&content) {
                    Ok(schema_json) => {
                        // Validate that it's a proper JSON Schema by attempting to compile it
                        match jsonschema::validator_for(&schema_json) {
                            Ok(_validator) => CheckResult {
                                name: "schema_valid".to_string(),
                                status: CheckStatus::Passed,
                                message: "JSON schema is valid and compilable".to_string(),
                                details: None,
                                execution_time_ms: start_time.elapsed().as_millis() as u64,
                            },
                            Err(e) => CheckResult {
                                name: "schema_valid".to_string(),
                                status: CheckStatus::Failed,
                                message: format!("Invalid JSON Schema: {e}"),
                                details: Some(
                                    serde_json::json!({"error": format!("Schema compilation failed: {}", e)}),
                                ),
                                execution_time_ms: start_time.elapsed().as_millis() as u64,
                            },
                        }
                    }
                    Err(e) => CheckResult {
                        name: "schema_valid".to_string(),
                        status: CheckStatus::Failed,
                        message: format!("Invalid JSON in schema file: {e}"),
                        details: None,
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    },
                }
            }
            Err(e) => CheckResult {
                name: "schema_valid".to_string(),
                status: CheckStatus::Failed,
                message: format!("Failed to read schema file: {e}"),
                details: None,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
            },
        };

        // Update cache
        if let Ok(metadata) = tokio::fs::metadata(schema_path).await
            && let Ok(mtime) = metadata.modified()
        {
            self.schema_validation_cache
                .insert(schema_path.to_path_buf(), (mtime, result.clone()));
        }

        result
    }

    /// Test basic tool execution
    async fn test_tool_execution(&self, config: &CliToolConfig) -> CheckResult {
        let start_time = Instant::now();

        // Execute tool off the async executor — `Command::output()` blocks
        // until the subprocess exits. See the `# Blocking` docs in
        // `src/agent/runloop/git.rs` for the pattern.
        let exec_path = config.executable_path.clone();

        // Try --help first
        let output =
            tokio::task::spawn_blocking(move || std::process::Command::new(&exec_path).arg("--help").output()).await;

        match output {
            Ok(Ok(output)) if output.status.success() => CheckResult {
                name: "tool_executable".to_string(),
                status: CheckStatus::Passed,
                message: "Tool executed successfully with --help".to_string(),
                details: None,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
            },
            Ok(Ok(_)) => {
                // --help failed, try -h
                let exec_path = config.executable_path.clone();
                match tokio::task::spawn_blocking(move || std::process::Command::new(&exec_path).arg("-h").output())
                    .await
                {
                    Ok(Ok(output)) if output.status.success() => CheckResult {
                        name: "tool_executable".to_string(),
                        status: CheckStatus::Passed,
                        message: "Tool executed successfully with -h".to_string(),
                        details: None,
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    },
                    Ok(Ok(_)) => CheckResult {
                        name: "tool_executable".to_string(),
                        status: CheckStatus::Warning,
                        message: "Tool executed but returned non-zero exit code".to_string(),
                        details: None,
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    },
                    Ok(Err(e)) => CheckResult {
                        name: "tool_executable".to_string(),
                        status: CheckStatus::Failed,
                        message: format!("Failed to execute tool: {e}"),
                        details: None,
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    },
                    Err(e) => CheckResult {
                        name: "tool_executable".to_string(),
                        status: CheckStatus::Failed,
                        message: format!("Validation task failed: {e}"),
                        details: None,
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    },
                }
            }
            Ok(Err(e)) => CheckResult {
                name: "tool_executable".to_string(),
                status: CheckStatus::Failed,
                message: format!("Failed to execute tool: {e}"),
                details: None,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
            },
            Err(e) => CheckResult {
                name: "tool_executable".to_string(),
                status: CheckStatus::Failed,
                message: format!("Validation task failed: {e}"),
                details: None,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
            },
        }
    }

    /// Assess security based on checks
    fn assess_security(&self, checks: &HashMap<String, CheckResult>) -> SecurityAssessment {
        let mut warnings = vec![];
        let blocked_content = vec![];
        let mut security_level = SecurityLevel::Safe;

        // Check for script security issues
        if let Some(scripts_check) = checks.get("scripts_valid")
            && scripts_check.status == CheckStatus::Warning
            && let Some(details) = &scripts_check.details
            && let Some(issues) = details.as_array()
        {
            for issue in issues {
                if let Some(issue_str) = issue.as_str()
                    && issue_str.contains("dangerous")
                {
                    warnings.push(SecurityWarning {
                        warning_type: "dangerous_content".to_string(),
                        message: issue_str.to_string(),
                        severity: SecurityLevel::HighRisk,
                        suggestion: Some("Review script content for security issues".to_string()),
                    });
                    security_level = SecurityLevel::HighRisk;
                }
            }
        }

        let safe_to_execute = security_level != SecurityLevel::HighRisk;

        SecurityAssessment {
            security_level,
            warnings,
            blocked_content,
            safe_to_execute,
        }
    }

    /// Generate recommendations based on validation results
    fn generate_recommendations(
        &self,
        checks: &HashMap<String, CheckResult>,
        security: &SecurityAssessment,
    ) -> Vec<String> {
        let mut recommendations = vec![];

        // General recommendations based on check results
        for check in checks.values() {
            match check.status {
                CheckStatus::Warning => {
                    recommendations.push(format!("Address warning in {}: {}", check.name, check.message));
                }
                CheckStatus::Failed => {
                    recommendations.push(format!("Fix failed check {}: {}", check.name, check.message));
                }
                _ => {}
            }
        }

        // Security recommendations
        if security.security_level == SecurityLevel::HighRisk {
            recommendations.push("Review and fix security issues before using this skill".to_string());
        }

        // Performance recommendations
        if let Some(loading_check) = checks.get("skill_file_valid")
            && loading_check.execution_time_ms > 1000
        {
            recommendations.push("Consider optimizing skill file parsing performance".to_string());
        }

        recommendations
    }

    /// Determine overall validation status
    fn determine_overall_status(
        &self,
        checks: &HashMap<String, CheckResult>,
        security: &SecurityAssessment,
    ) -> ValidationStatus {
        let has_failures = checks.values().any(|check| check.status == CheckStatus::Failed);
        let has_warnings = checks.values().any(|check| check.status == CheckStatus::Warning);
        let has_high_risk = security.security_level == SecurityLevel::HighRisk;

        if has_failures || has_high_risk {
            ValidationStatus::Invalid
        } else if has_warnings {
            ValidationStatus::Warning
        } else {
            ValidationStatus::Valid
        }
    }

    /// Validate multiple skills in batch
    pub async fn validate_batch(&mut self, skill_paths: Vec<&Path>) -> Vec<Result<ValidationReport>> {
        let mut results = vec![];

        for path in skill_paths {
            let result = self.validate_skill_directory(path).await;
            results.push(result);
        }

        results
    }
}

/// Batch validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchValidationResult {
    /// Total skills validated
    pub total_skills: usize,

    /// Valid skills
    pub valid_skills: Vec<String>,

    /// Skills with warnings
    pub warning_skills: Vec<String>,

    /// Invalid skills
    pub invalid_skills: Vec<String>,

    /// Average validation time
    pub average_validation_time_ms: u64,

    /// Validation reports
    pub reports: Vec<ValidationReport>,
}

/// Validate a batch of skills and summarize results
pub async fn validate_skill_batch(skill_paths: Vec<&Path>) -> Result<BatchValidationResult> {
    let mut validator = SkillValidator::new();
    let mut reports = vec![];
    let mut total_time = 0u64;

    for path in skill_paths {
        match validator.validate_skill_directory(path).await {
            Ok(report) => {
                total_time += report.performance.total_time_ms;
                reports.push(report);
            }
            Err(e) => {
                // Create error report
                let error_report = ValidationReport {
                    status: ValidationStatus::Invalid,
                    skill_name: path.to_string_lossy().to_string(),
                    timestamp: chrono::Utc::now(),
                    checks: HashMap::new(),
                    performance: PerformanceMetrics::default(),
                    security: SecurityAssessment::default(),
                    recommendations: vec![format!("Validation failed: {}", e)],
                };
                reports.push(error_report);
            }
        }
    }

    let valid_skills: Vec<String> = reports
        .iter()
        .filter(|r| r.status == ValidationStatus::Valid)
        .map(|r| r.skill_name.clone())
        .collect();

    let warning_skills: Vec<String> = reports
        .iter()
        .filter(|r| r.status == ValidationStatus::Warning)
        .map(|r| r.skill_name.clone())
        .collect();

    let invalid_skills: Vec<String> = reports
        .iter()
        .filter(|r| r.status == ValidationStatus::Invalid)
        .map(|r| r.skill_name.clone())
        .collect();

    let average_time = if !reports.is_empty() {
        total_time / reports.len() as u64
    } else {
        0
    };

    Ok(BatchValidationResult {
        total_skills: reports.len(),
        valid_skills,
        warning_skills,
        invalid_skills,
        average_validation_time_ms: average_time,
        reports,
    })
}

#[cfg(test)]
mod tests;
