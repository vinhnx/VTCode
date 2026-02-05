#[cfg(test)]
mod validation_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_manifest_validation_consecutive_hyphens() {
        let m = SkillManifest {
            name: "test--skill".to_string(),
            description: "Valid description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            argument_hint: None,
            user_invocable: None,
            context: None,
            agent: None,
            hooks: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
            tools: None,
        };
        let result = m.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("consecutive hyphens"));
    }

    #[test]
    fn test_manifest_validation_leading_hyphen() {
        let m = SkillManifest {
            name: "-test-skill".to_string(),
            description: "Valid description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            argument_hint: None,
            user_invocable: None,
            context: None,
            agent: None,
            hooks: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
            tools: None,
        };
        let result = m.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("starts with hyphen"));
    }

    #[test]
    fn test_manifest_validation_trailing_hyphen() {
        let m = SkillManifest {
            name: "test-skill-".to_string(),
            description: "Valid description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            argument_hint: None,
            user_invocable: None,
            context: None,
            agent: None,
            hooks: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
            tools: None,
        };
        let result = m.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("ends with hyphen"));
    }

    #[test]
    fn test_manifest_validation_empty_description() {
        let m = SkillManifest {
            name: "test-skill".to_string(),
            description: "".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            argument_hint: None,
            user_invocable: None,
            context: None,
            agent: None,
            hooks: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
            tools: None,
        };
        let result = m.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("description is required"));
    }

    #[test]
    fn test_directory_name_match_success() {
        let temp_dir = TempDir::new().unwrap();
        let skill_dir = temp_dir.path().join("test-skill");
        fs::create_dir(&skill_dir).unwrap();
        
        let skill_md = skill_dir.join("SKILL.md");
        
        let m = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            argument_hint: None,
            user_invocable: None,
            context: None,
            agent: None,
            hooks: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
            tools: None,
        };
        
        let result = m.validate_directory_name_match(&skill_md);
        assert!(result.is_ok());
    }

    #[test]
    fn test_directory_name_match_failure() {
        let temp_dir = TempDir::new().unwrap();
        let skill_dir = temp_dir.path().join("different-name");
        fs::create_dir(&skill_dir).unwrap();
        
        let skill_md = skill_dir.join("SKILL.md");
        
        let m = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            argument_hint: None,
            user_invocable: None,
            context: None,
            agent: None,
            hooks: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
            tools: None,
        };
        
        let result = m.validate_directory_name_match(&skill_md);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("does not match directory name"));
    }

    #[test]
    fn test_directory_name_match_cli_tool_skipped() {
        let temp_dir = TempDir::new().unwrap();
        let tool_dir = temp_dir.path().join("my-tool");
        fs::create_dir(&tool_dir).unwrap();
        
        // Create a tool.json to indicate this is a CLI tool
        fs::write(tool_dir.join("tool.json"), r#"{"name": "test-tool"}"#).unwrap();
        
        let skill_md = tool_dir.join("SKILL.md");
        
        let m = SkillManifest {
            name: "test-tool".to_string(),
            description: "Test tool description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            argument_hint: None,
            user_invocable: None,
            context: None,
            agent: None,
            hooks: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
            tools: None,
        };
        
        // CLI tools should skip directory name validation
        let result = m.validate_directory_name_match(&skill_md);
        assert!(result.is_ok());
    }
}
