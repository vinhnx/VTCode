//! Integration tests for MCP code execution architecture
//!
//! Tests all 5 steps from Anthropic's code execution recommendations:
//! 1. Progressive tool discovery
//! 2. Code executor with SDK generation
//! 3. Skill persistence
//! 4. Data filtering in code
//! 5. PII tokenization

#[cfg(test)]
mod tests {
    use crate::exec::{
        AgentBehaviorAnalyzer, CodeExecutor, ExecutionConfig, Language, SkillManager, PiiTokenizer, PiiType,
    };
    use std::path::PathBuf;

    // ============================================================================
    // Test 1: Discovery → Execution → Filtering
    // ============================================================================

    #[test]
    fn test_discovery_to_execution_flow() {
        // This test validates that tool discovery results can feed into code execution
        // In real usage: agents discover tools, then use them in written code

        // Step 1: Simulate tool discovery by initializing a code executor
        // (which has built-in knowledge of available tools)
        let config = ExecutionConfig {
            timeout_secs: 5,
            max_memory_mb: 256,
            ..Default::default()
        };

        let executor = CodeExecutor::new(config);
        assert!(executor.is_ok());

        // Step 2: Agent writes code that filters data locally
        let code = r#"
# Simulate filtering without returning all results to model
data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
filtered = [x for x in data if x > 5]
result = {"count": len(filtered), "items": filtered}
"#;

        // Step 3: Execute code with filtering
        // (actual code runs locally, only aggregated result returns to model)
        let executor = executor.unwrap();
        let result = executor.execute(code, Language::Python3).unwrap();

        // Step 4: Verify filtering happened locally
        assert_eq!(result.exit_code, 0);
        assert!(result.result.is_some());
        let result_obj = result.result.unwrap();
        assert_eq!(result_obj.get("count").and_then(|v| v.as_u64()), Some(5));
    }

    // ============================================================================
    // Test 2: Execution → Skill Persistence → Reuse
    // ============================================================================

    #[test]
    fn test_execution_to_skill_reuse() {
        // Create temporary directory for skills
        let temp_dir = std::env::temp_dir().join("vtcode_test_skills");
        let _ = std::fs::create_dir_all(&temp_dir);

        // Step 1: Execute and test code
        let config = ExecutionConfig {
            timeout_secs: 5,
            max_memory_mb: 256,
            ..Default::default()
        };
        let executor = CodeExecutor::new(config).unwrap();

        let code = r#"
def double_value(x):
    return x * 2

result = {"test": double_value(21)}
"#;

        let execution_result = executor.execute(code, Language::Python3).unwrap();
        assert_eq!(execution_result.exit_code, 0);

        // Step 2: Save as skill
        let mut skill_manager = SkillManager::new(temp_dir.clone()).unwrap();

        skill_manager
            .save_skill(
                "double_value",
                Language::Python3,
                code.to_string(),
                Some("Double a number".to_string()),
                None,
                None,
                Some(vec!["math".to_string()]),
            )
            .unwrap();

        // Step 3: Verify skill was saved
        let skills = skill_manager.list_skills().unwrap();
        assert!(skills.iter().any(|s| s.name == "double_value"));

        // Step 4: Load and reuse skill
        let skill = skill_manager.load_skill("double_value").unwrap();
        assert_eq!(skill.name, "double_value");
        assert_eq!(skill.language, Language::Python3);

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // ============================================================================
    // Test 3: PII Protection in Pipeline
    // ============================================================================

    #[test]
    fn test_pii_protection_in_execution() {
        // Create a PII tokenizer
        let mut tokenizer = PiiTokenizer::new();

        // Step 1: Detect PII patterns
        let text_with_pii = "Email: john@example.com, SSN: 123-45-6789";

        let detected = tokenizer.detect_pii(text_with_pii).unwrap();
        assert!(!detected.is_empty());

        // Step 2: Verify we can tokenize
        let (tokenized, _tokens) = tokenizer.tokenize_string(text_with_pii).unwrap();

        // Step 3: Verify tokenized version doesn't contain plaintext PII
        assert!(!tokenized.contains("john@example.com"));
        assert!(!tokenized.contains("123-45-6789"));
        assert!(tokenized.contains("__PII_"));

        // Step 4: Verify we can detokenize
        let detokenized = tokenizer.detokenize_string(&tokenized).unwrap();
        assert!(detokenized.contains("john@example.com"));
        assert!(detokenized.contains("123-45-6789"));
    }

    // ============================================================================
    // Test 4: Large Dataset Filtering
    // ============================================================================

    #[test]
    fn test_large_dataset_filtering_efficiency() {
        let config = ExecutionConfig {
            timeout_secs: 5,
            max_memory_mb: 256,
            ..Default::default()
        };
        let executor = CodeExecutor::new(config).unwrap();

        // Code that filters large dataset locally (simulated)
        let code = r#"
# Simulate processing large dataset
items = list(range(1000))

# Filter in code (not returned to model)
filtered_items = [x for x in items if x % 10 == 0]
stats = {
    "total": len(items),
    "filtered": len(filtered_items),
    "sample": filtered_items[:5]
}

result = stats
"#;

        let result = executor.execute(code, Language::Python3).unwrap();

        // Verify result contains only filtered summary
        assert_eq!(result.exit_code, 0);
        let result_obj = result.result.unwrap();
        assert_eq!(result_obj.get("total").and_then(|v| v.as_u64()), Some(1000));
        assert_eq!(result_obj.get("filtered").and_then(|v| v.as_u64()), Some(100));

        // Verify only sample returned, not all 1000 items
        if let Some(sample) = result_obj.get("sample").and_then(|v| v.as_array()) {
            assert!(sample.len() <= 5);
        }
    }

    // ============================================================================
    // Test 5: Tool Error Handling in Code
    // ============================================================================

    #[test]
    fn test_tool_error_handling_in_code() {
        let config = ExecutionConfig {
            timeout_secs: 5,
            max_memory_mb: 256,
            ..Default::default()
        };
        let executor = CodeExecutor::new(config).unwrap();

        // Code with error handling
        let code = r#"
try:
    # This would fail if we tried to access nonexistent file
    # For now, simulate an error
    x = 1 / 0  # Division by zero
    result = {"error": False}
except ZeroDivisionError as e:
    result = {"error": True, "type": "ZeroDivisionError", "message": str(e)}
except Exception as e:
    result = {"error": True, "type": type(e).__name__, "message": str(e)}
"#;

        let result = executor.execute(code, Language::Python3).unwrap();

        // Code should execute successfully (error handling worked)
        assert_eq!(result.exit_code, 0);
        let result_obj = result.result.unwrap();
        assert_eq!(result_obj.get("error").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            result_obj
                .get("type")
                .and_then(|v| v.as_str()),
            Some("ZeroDivisionError")
        );
    }

    // ============================================================================
    // Test 6: Agent Behavior Analysis
    // ============================================================================

    #[test]
    fn test_agent_behavior_tracking() {
        let mut analyzer = AgentBehaviorAnalyzer::new();

        // Record tool usage
        analyzer.record_tool_usage("list_files");
        analyzer.record_tool_usage("list_files");
        analyzer.record_tool_usage("read_file");

        // Record skill reuse
        analyzer.record_skill_reuse("filter_skill");
        analyzer.record_skill_reuse("filter_skill");

        // Record failures
        analyzer.record_tool_failure("grep_tool", "timeout");
        analyzer.record_tool_failure("grep_tool", "pattern_error");

        // Verify statistics
        assert_eq!(analyzer.tool_stats().usage_frequency.get("list_files"), Some(&2));
        assert_eq!(analyzer.skill_stats().reused_skills, 2);
        assert!(!analyzer.failure_patterns().high_failure_tools.is_empty());

        // Get recommendations
        let tool_recs = analyzer.recommend_tools("list", 1);
        assert!(tool_recs.contains(&"list_files".to_string()));

        // Identify risky tools
        let risky = analyzer.identify_risky_tools(0.3);
        assert!(!risky.is_empty());
    }

    // ============================================================================
    // Scenario Tests
    // ============================================================================

    #[test]
    fn test_scenario_simple_transformation() {
        let config = ExecutionConfig {
            timeout_secs: 5,
            max_memory_mb: 256,
            ..Default::default()
        };
        let executor = CodeExecutor::new(config).unwrap();

        let code = r#"
# Transform data locally before returning
data = ["hello", "world", "test"]
transformed = [s.upper() for s in data]
result = {"original_count": len(data), "transformed": transformed}
"#;

        let result = executor.execute(code, Language::Python3).unwrap();
        assert_eq!(result.exit_code, 0);

        let result_obj = result.result.unwrap();
        assert_eq!(result_obj.get("original_count").and_then(|v| v.as_u64()), Some(3));
        if let Some(transformed) = result_obj.get("transformed").and_then(|v| v.as_array()) {
            assert_eq!(transformed.len(), 3);
        }
    }

    #[test]
    fn test_javascript_execution() {
        let config = ExecutionConfig {
            timeout_secs: 5,
            max_memory_mb: 256,
            ..Default::default()
        };
        let executor = CodeExecutor::new(config).unwrap();

        let code = r#"
const items = [1, 2, 3, 4, 5];
const filtered = items.filter(x => x > 2);
result = { count: filtered.length, items: filtered };
"#;

        let result = executor.execute(code, Language::JavaScript).unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.result.is_some());
    }
}
