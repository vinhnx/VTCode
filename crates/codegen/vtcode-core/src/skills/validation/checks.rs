use super::*;

impl SkillValidator {
    /// Check if directory exists
    pub(super) async fn check_directory_exists(&self, path: &Path) -> CheckResult {
        let start_time = Instant::now();

        let status = if tokio::fs::metadata(path).await.is_ok_and(|metadata| metadata.is_dir()) {
            CheckStatus::Passed
        } else {
            CheckStatus::Failed
        };

        let message = if status == CheckStatus::Passed {
            format!("Directory exists: {}", path.display())
        } else {
            format!("Directory does not exist: {}", path.display())
        };

        CheckResult {
            name: "directory_exists".to_string(),
            status,
            message,
            details: None,
            execution_time_ms: start_time.elapsed().as_millis() as u64,
        }
    }

    /// Validate SKILL.md file
    pub(super) async fn validate_skill_file(&mut self, skill_file: &Path) -> CheckResult {
        let start_time = Instant::now();

        if !skill_file.exists() {
            return CheckResult {
                name: "skill_file_valid".to_string(),
                status: CheckStatus::Failed,
                message: format!("SKILL.md file not found: {}", skill_file.display()),
                details: None,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
            };
        }

        match parse_skill_file(skill_file.parent().unwrap_or_else(|| Path::new("."))) {
            Ok((manifest, _instructions)) => {
                if let Err(err) = manifest.validate() {
                    let mut details = serde_json::Map::new();
                    details.insert("name".to_string(), Value::String(manifest.name.clone()));
                    details.insert("description".to_string(), Value::String(manifest.description.clone()));
                    return CheckResult {
                        name: "skill_file_valid".to_string(),
                        status: CheckStatus::Failed,
                        message: format!("SKILL.md validation failed: {err}"),
                        details: Some(Value::Object(details)),
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    };
                }

                // Validate required fields
                let mut warnings = vec![];

                for field in &self.config.required_metadata_fields {
                    match field.as_str() {
                        "name" => {
                            if manifest.name.is_empty() {
                                warnings.push("Skill name is empty");
                            }
                        }
                        "description" if manifest.description.is_empty() => {
                            warnings.push("Skill description is empty");
                        }
                        _ => {}
                    }
                }

                let status = if warnings.is_empty() {
                    CheckStatus::Passed
                } else {
                    CheckStatus::Warning
                };

                let message = if status == CheckStatus::Passed {
                    format!("SKILL.md is valid: {}", manifest.name)
                } else {
                    format!("SKILL.md has warnings: {}", warnings.join(", "))
                };

                let mut details = serde_json::Map::new();
                details.insert("name".to_string(), Value::String(manifest.name.clone()));
                details.insert("description".to_string(), Value::String(manifest.description.clone()));
                details.insert(
                    "warnings".to_string(),
                    serde_json::to_value(&warnings).unwrap_or_else(|_| Value::Array(vec![])),
                );

                CheckResult {
                    name: "skill_file_valid".to_string(),
                    status,
                    message,
                    details: Some(Value::Object(details)),
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                }
            }
            Err(e) => CheckResult {
                name: "skill_file_valid".to_string(),
                status: CheckStatus::Failed,
                message: format!("Failed to parse SKILL.md: {e:#}"),
                details: None,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
            },
        }
    }

    /// Validate scripts directory
    pub(super) async fn validate_scripts_directory(&self, scripts_dir: &Path) -> CheckResult {
        let start_time = Instant::now();
        let mut issues = vec![];

        let mut entries = match tokio::fs::read_dir(scripts_dir).await {
            Ok(entries) => entries,
            Err(error) => {
                return CheckResult {
                    name: "scripts_valid".to_string(),
                    status: CheckStatus::Failed,
                    message: format!("Failed to read scripts directory {}: {}", scripts_dir.display(), error),
                    details: None,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                };
            }
        };
        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(e) => {
                    return CheckResult {
                        name: "scripts_valid".to_string(),
                        status: CheckStatus::Failed,
                        message: format!("Failed to read scripts directory entry: {e}"),
                        details: None,
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    };
                }
            };
            let path = entry.path();
            if entry.file_type().await.is_ok_and(|file_type| file_type.is_file()) {
                // Check file size
                if let Some(metadata) = entry
                    .metadata()
                    .await
                    .ok()
                    .filter(|m| m.len() > self.config.max_script_size as u64)
                {
                    issues.push(format!("Script too large: {} ({} bytes)", path.display(), metadata.len()));
                }

                // Check extension
                if let Some(ext) = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .filter(|e| !self.config.allowed_script_extensions.contains(&e.to_string()))
                {
                    issues.push(format!("Unsupported script type: {ext}"));
                }

                // Security check
                if self.config.enable_security_checks
                    && let Ok(content) = read_file_with_context_sync(&path, "skill script")
                {
                    for blocked in &self.config.blocked_commands {
                        if content.contains(blocked) {
                            issues.push(format!("Potentially dangerous content found: {blocked}"));
                        }
                    }
                }
            }
        }

        let status = if issues.is_empty() {
            CheckStatus::Passed
        } else {
            CheckStatus::Warning
        };

        let message = if status == CheckStatus::Passed {
            "Scripts directory is valid".to_string()
        } else {
            format!("Scripts directory has issues: {}", issues.join(", "))
        };

        CheckResult {
            name: "scripts_valid".to_string(),
            status,
            message,
            details: Some(serde_json::to_value(&issues).unwrap_or_else(|_| Value::Array(vec![]))),
            execution_time_ms: start_time.elapsed().as_millis() as u64,
        }
    }

    /// Validate resources
    pub(super) async fn validate_resources(&self, skill_path: &Path) -> HashMap<String, CheckResult> {
        let mut results = HashMap::new();

        // Check for common resource directories
        for resource_dir in &["templates", "data", "config"] {
            let dir_path = skill_path.join(resource_dir);
            if dir_path.exists() {
                let result = self.validate_resource_directory(&dir_path, resource_dir).await;
                results.insert(resource_dir.to_string(), result);
            }
        }

        results
    }

    /// Validate resource directory
    pub(super) async fn validate_resource_directory(&self, dir_path: &Path, resource_type: &str) -> CheckResult {
        let start_time = Instant::now();

        let mut issues = vec![];

        let mut entries = match tokio::fs::read_dir(dir_path).await {
            Ok(entries) => entries,
            Err(error) => {
                return CheckResult {
                    name: format!("resource_{resource_type}"),
                    status: CheckStatus::Failed,
                    message: format!("Failed to read resource directory {}: {}", dir_path.display(), error),
                    details: None,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                };
            }
        };
        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(e) => {
                    return CheckResult {
                        name: format!("resource_{resource_type}"),
                        status: CheckStatus::Failed,
                        message: format!("Failed to read resource directory entry: {e}"),
                        details: None,
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    };
                }
            };
            let path = entry.path();
            if entry.file_type().await.is_ok_and(|file_type| file_type.is_file()) {
                // Check file size
                if let Some(metadata) = entry.metadata().await.ok().filter(|m| m.len() > 10 * 1024 * 1024) {
                    // 10MB limit for resources
                    issues.push(format!("Resource file too large: {} ({} bytes)", path.display(), metadata.len()));
                }
            }
        }

        let status = if issues.is_empty() {
            CheckStatus::Passed
        } else {
            CheckStatus::Warning
        };

        let message = if status == CheckStatus::Passed {
            format!("{resource_type} directory is valid")
        } else {
            format!("{} directory has issues: {}", resource_type, issues.join(", "))
        };

        CheckResult {
            name: format!("resource_{resource_type}"),
            status,
            message,
            details: Some(serde_json::to_value(&issues).unwrap_or_else(|_| Value::Array(vec![]))),
            execution_time_ms: start_time.elapsed().as_millis() as u64,
        }
    }

    /// Check if executable exists
    pub(super) async fn check_executable_exists(&self, path: &Path) -> CheckResult {
        let start_time = Instant::now();

        let status = if tokio::fs::try_exists(path).await.unwrap_or(false) {
            CheckStatus::Passed
        } else {
            CheckStatus::Failed
        };

        let message = if status == CheckStatus::Passed {
            format!("Executable exists: {}", path.display())
        } else {
            format!("Executable not found: {}", path.display())
        };

        CheckResult {
            name: "executable_exists".to_string(),
            status,
            message,
            details: None,
            execution_time_ms: start_time.elapsed().as_millis() as u64,
        }
    }

    /// Check executable permissions
    #[cfg_attr(
        not(unix),
        allow(
            unused_variables,
            reason = "Intentional compatibility, platform, or test-only suppression."
        )
    )]
    pub(super) async fn check_executable_permissions(&self, path: &Path) -> CheckResult {
        let start_time = Instant::now();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Ok(metadata) = tokio::fs::metadata(path).await {
                let permissions = metadata.permissions();
                let is_executable = permissions.mode() & 0o111 != 0;

                let status = if is_executable {
                    CheckStatus::Passed
                } else {
                    CheckStatus::Failed
                };

                let message = if status == CheckStatus::Passed {
                    "Executable has proper permissions".to_string()
                } else {
                    "Executable lacks execute permissions".to_string()
                };

                return CheckResult {
                    name: "executable_permissions".to_string(),
                    status,
                    message,
                    details: None,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                };
            }
        }

        // Windows or metadata error - assume valid
        CheckResult {
            name: "executable_permissions".to_string(),
            status: CheckStatus::Passed,
            message: "Permission check skipped (Windows or metadata error)".to_string(),
            details: None,
            execution_time_ms: start_time.elapsed().as_millis() as u64,
        }
    }

    /// Validate README file
    pub(super) async fn validate_readme_file(&self, readme_path: &Path) -> CheckResult {
        let start_time = Instant::now();

        if !readme_path.exists() {
            return CheckResult {
                name: "readme_valid".to_string(),
                status: CheckStatus::Warning,
                message: "README file not found".to_string(),
                details: None,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
            };
        }

        match read_file_with_context_sync(readme_path, "skill README") {
            Ok(content) => {
                if content.len() < 100 {
                    CheckResult {
                        name: "readme_valid".to_string(),
                        status: CheckStatus::Warning,
                        message: "README file is very short".to_string(),
                        details: Some(serde_json::json!({"length": content.len()})),
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    }
                } else {
                    CheckResult {
                        name: "readme_valid".to_string(),
                        status: CheckStatus::Passed,
                        message: "README file is valid".to_string(),
                        details: Some(serde_json::json!({"length": content.len()})),
                        execution_time_ms: start_time.elapsed().as_millis() as u64,
                    }
                }
            }
            Err(e) => CheckResult {
                name: "readme_valid".to_string(),
                status: CheckStatus::Failed,
                message: format!("Failed to read README: {e}"),
                details: None,
                execution_time_ms: start_time.elapsed().as_millis() as u64,
            },
        }
    }
}
