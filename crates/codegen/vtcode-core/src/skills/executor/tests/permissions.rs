use super::*;

#[test]
fn test_skill_execution_context() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };

    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "Instructions".to_string()).expect("failed to create skill");

    let tools = vec!["file_ops".to_string(), "shell".to_string()];
    let input = serde_json::json!({"test": "input"});

    let ctx = SkillExecutionContext::new(&skill, input, tools);
    assert_eq!(ctx.skill_name, "test-skill");
    assert_eq!(ctx.available_tools.len(), 2);
}
#[test]
fn skill_command_permissions_inject_additional_permissions() {
    let skill = test_skill_with_permissions(Some(SkillPermissionProfile {
        file_system: Some(
            SkillFileSystemPermissions {
                read: vec![PathBuf::from("references")],
                write: vec![PathBuf::from("outputs")],
            }
            .into(),
        ),
    }));

    let merged = merge_skill_command_permissions(&skill, "shell", serde_json::json!({"command": "pwd"}));

    assert_eq!(merged["sandbox_permissions"], serde_json::json!("with_additional_permissions"));
    assert_eq!(merged["additional_permissions"]["fs_read"], serde_json::json!(["/tmp/test-skill/references"]));
    assert_eq!(merged["additional_permissions"]["fs_write"], serde_json::json!(["/tmp/test-skill/outputs"]));
}
#[test]
fn skill_command_permissions_merge_existing_permissions() {
    let skill = test_skill_with_permissions(Some(SkillPermissionProfile {
        file_system: Some(
            SkillFileSystemPermissions {
                read: vec![PathBuf::from("references")],
                write: vec![PathBuf::from("outputs")],
            }
            .into(),
        ),
    }));

    let merged = merge_skill_command_permissions(
        &skill,
        "shell",
        serde_json::json!({
            "command": "pwd",
            "sandbox_permissions": "with_additional_permissions",
            "additional_permissions": {
                "fs_read": ["/tmp/existing-read"],
                "fs_write": ["/tmp/existing-write"]
            }
        }),
    );

    assert_eq!(
        merged["additional_permissions"]["fs_read"],
        serde_json::json!(["/tmp/existing-read", "/tmp/test-skill/references"])
    );
    assert_eq!(
        merged["additional_permissions"]["fs_write"],
        serde_json::json!(["/tmp/existing-write", "/tmp/test-skill/outputs"])
    );
}
#[test]
fn skill_command_permissions_ignore_require_escalated() {
    let skill = test_skill_with_permissions(Some(SkillPermissionProfile {
        file_system: Some(
            SkillFileSystemPermissions {
                read: Vec::new(),
                write: vec![PathBuf::from("outputs")],
            }
            .into(),
        ),
    }));
    let original = serde_json::json!({
        "command": "pwd",
        "sandbox_permissions": "require_escalated",
        "justification": "Do you want to run this command without sandbox restrictions?"
    });

    let merged = merge_skill_command_permissions(&skill, "shell", original.clone());

    assert_eq!(merged, original);
}
#[test]
fn skill_command_permissions_ignore_empty_skill_permissions() {
    let skill = test_skill_with_permissions(None);
    let original = serde_json::json!({"command": "pwd"});

    let merged = merge_skill_command_permissions(&skill, "shell", original.clone());

    assert_eq!(merged, original);
}
