use super::*;
use tempfile::TempDir;

#[test]
fn test_validation_config_default() {
    let config = ValidationConfig::default();
    assert!(config.enable_security_checks);
    assert!(config.enable_performance_checks);
    assert_eq!(config.max_validation_time, 30);
}

#[tokio::test]
async fn test_validator_creation() {
    let _validator = SkillValidator::new();
    // assert_eq!(validator.schema_cache.len(), 0); // Commented out since schema_cache is disabled
}

#[tokio::test]
async fn test_invalid_skill_directory() {
    let mut validator = SkillValidator::new();
    let temp_dir = TempDir::new().unwrap();
    let non_existent = temp_dir.path().join("non_existent");

    let result = validator.validate_skill_directory(&non_existent).await;
    assert!(result.is_ok());

    let report = result.unwrap();
    assert_eq!(report.status, ValidationStatus::Invalid);
}

#[tokio::test]
async fn test_skill_validation_rejects_hooks() {
    let temp_dir = TempDir::new().unwrap();
    let skill_dir = temp_dir.path().join("hook-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();

    let skill_md = r#"---
name: hook-skill
description: Skill with hooks
hooks:
  pre_tool_use:
    - command: "echo pre"
---
# Hook Skill
"#;
    std::fs::write(skill_dir.join("SKILL.md"), skill_md).unwrap();

    let mut validator = SkillValidator::new();
    let report = validator.validate_skill_directory(&skill_dir).await.unwrap();

    assert_eq!(report.status, ValidationStatus::Invalid);
    let check = report.checks.get("skill_file_valid").unwrap();
    assert_eq!(check.status, CheckStatus::Failed);
    assert!(check.message.contains("hooks"));
}

#[tokio::test]
async fn test_schema_validation_caching() {
    use std::fs::File;
    use std::io::Write;

    let temp_dir = TempDir::new().unwrap();
    let schema_path = temp_dir.path().join("schema.json");

    // precise sleep to ensure file system time resolution
    let sleep_fs = || std::thread::sleep(std::time::Duration::from_millis(50));

    // Create initial schema
    {
        let mut file = File::create(&schema_path).unwrap();
        write!(file, r#"{{"type": "string"}}"#).unwrap();
    }
    sleep_fs();

    let mut validator = SkillValidator::new();

    // 1. First validation - should cache
    let result1 = validator.validate_json_schema(&schema_path).await;
    assert_eq!(result1.status, CheckStatus::Passed);
    assert_eq!(validator.schema_validation_cache.len(), 1);

    // Capture mtime in cache
    let (cached_mtime, _) = validator.schema_validation_cache.get(&schema_path).unwrap();
    let cached_mtime = *cached_mtime;

    // 2. Second validation - should hit cache (mtime same)
    let result2 = validator.validate_json_schema(&schema_path).await;
    assert_eq!(result2.status, CheckStatus::Passed);
    // Verify we still have the same cache entry
    assert_eq!(validator.schema_validation_cache.get(&schema_path).unwrap().0, cached_mtime);

    // 3. Modify file - should invalidate cache
    sleep_fs();
    {
        let mut file = File::create(&schema_path).unwrap();
        write!(file, r#"{{"type": "integer"}}"#).unwrap();
    }
    sleep_fs();

    let result3 = validator.validate_json_schema(&schema_path).await;
    assert_eq!(result3.status, CheckStatus::Passed);

    let (new_mtime, _) = validator.schema_validation_cache.get(&schema_path).unwrap();
    assert_ne!(*new_mtime, cached_mtime, "Cache should have updated with new mtime");
}
