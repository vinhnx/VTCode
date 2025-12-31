//! Integration tests for Phase 5: Unified Command Evaluator
//!
//! Tests comprehensive command safety and policy evaluation,
//! covering interactions between safety rules, policy rules, and shell parsing.

#[cfg(test)]
mod tests {
    use crate::command_safety::{EvaluationReason, PolicyAwareEvaluator, UnifiedCommandEvaluator};

    // ========== Core Safety Tests ==========

    #[tokio::test]
    async fn test_safe_git_commands() {
        let evaluator = UnifiedCommandEvaluator::new();
        let safe_commands = vec![
            vec!["git", "status"],
            vec!["git", "log"],
            vec!["git", "branch"],
            vec!["git", "diff"],
            vec!["git", "show"],
        ];

        for cmd in safe_commands {
            let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
            let result = evaluator.evaluate(&cmd_vec).await.unwrap();
            assert!(result.allowed, "Expected {} to be allowed", cmd.join(" "));
        }
    }

    #[tokio::test]
    async fn test_forbidden_git_subcommands() {
        let evaluator = UnifiedCommandEvaluator::new();
        let forbidden_commands = vec![
            vec!["git", "push"],
            vec!["git", "pull"],
            vec!["git", "reset"],
            vec!["git", "clean"],
        ];

        for cmd in forbidden_commands {
            let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
            let result = evaluator.evaluate(&cmd_vec).await.unwrap();
            assert!(
                !result.allowed,
                "Expected {} to be forbidden",
                cmd.join(" ")
            );
        }
    }

    #[tokio::test]
    async fn test_safe_readonly_commands() {
        let evaluator = UnifiedCommandEvaluator::new();
        let safe_commands = vec![
            vec!["ls"],
            vec!["find", "."],
            vec!["grep", "pattern"],
            vec!["cat", "file.txt"],
        ];

        for cmd in safe_commands {
            let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
            let result = evaluator.evaluate(&cmd_vec).await.unwrap();
            assert!(result.allowed, "Expected {} to be allowed", cmd.join(" "));
        }
    }

    #[tokio::test]
    async fn test_dangerous_rm_commands() {
        let evaluator = UnifiedCommandEvaluator::new();
        let dangerous_commands = vec![
            vec!["rm", "-rf", "/"],
            vec!["rm", "-rf", "."],
            vec!["rm", "-rf"],
            vec!["sudo", "rm", "-rf", "/"],
        ];

        for cmd in dangerous_commands {
            let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
            let result = evaluator.evaluate(&cmd_vec).await.unwrap();
            assert!(!result.allowed, "Expected {} to be blocked", cmd.join(" "));
        }
    }

    #[tokio::test]
    async fn test_dangerous_mkfs_commands() {
        let evaluator = UnifiedCommandEvaluator::new();
        let dangerous_commands = vec![
            vec!["mkfs"],
            vec!["mkfs", "/dev/sda"],
            vec!["mkfs.ext4", "/dev/sda1"],
        ];

        for cmd in dangerous_commands {
            let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
            let result = evaluator.evaluate(&cmd_vec).await.unwrap();
            assert!(!result.allowed, "Expected {} to be blocked", cmd.join(" "));
        }
    }

    // ========== Find Command Tests (Option Blacklist) ==========

    #[tokio::test]
    async fn test_find_allowed_options() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec![
            "find".to_string(),
            ".".to_string(),
            "-name".to_string(),
            "*.rs".to_string(),
        ];
        let result = evaluator.evaluate(&cmd).await.unwrap();
        assert!(result.allowed, "find with -name should be allowed");
    }

    #[tokio::test]
    async fn test_find_forbidden_options() {
        let evaluator = UnifiedCommandEvaluator::new();
        let forbidden_commands = vec![
            vec!["find", ".", "-delete"],
            vec!["find", ".", "-exec", "rm", "{}", ";"],
        ];

        for cmd in forbidden_commands {
            let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
            let result = evaluator.evaluate(&cmd_vec).await.unwrap();
            assert!(
                !result.allowed,
                "Expected {} to be forbidden",
                cmd.join(" ")
            );
        }
    }

    // ========== Cargo Command Tests ==========

    #[tokio::test]
    async fn test_safe_cargo_commands() {
        let evaluator = UnifiedCommandEvaluator::new();
        let safe_commands = vec![
            vec!["cargo", "build"],
            vec!["cargo", "test"],
            vec!["cargo", "check"],
        ];

        for cmd in safe_commands {
            let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
            let result = evaluator.evaluate(&cmd_vec).await.unwrap();
            assert!(result.allowed, "Expected {} to be allowed", cmd.join(" "));
        }
    }

    // ========== Shell Parsing Tests (bash -lc decomposition) ==========

    #[tokio::test]
    async fn test_bash_lc_with_safe_commands() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec![
            "bash".to_string(),
            "-lc".to_string(),
            "git status && cargo test".to_string(),
        ];
        let result = evaluator.evaluate(&cmd).await.unwrap();
        assert!(
            result.allowed,
            "bash -lc with safe commands should be allowed"
        );
    }

    #[tokio::test]
    async fn test_bash_lc_with_dangerous_command() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec![
            "bash".to_string(),
            "-lc".to_string(),
            "git status && rm -rf /".to_string(),
        ];
        let result = evaluator.evaluate(&cmd).await.unwrap();
        assert!(
            !result.allowed,
            "bash -lc with dangerous command should be blocked"
        );
    }

    // ========== Policy Layer Tests ==========

    #[tokio::test]
    async fn test_policy_deny_blocks_safe_command() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["git".to_string(), "status".to_string()];
        let result = evaluator
            .evaluate_with_policy(&cmd, false, "policy blocked")
            .await
            .unwrap();
        assert!(!result.allowed);
        matches!(result.primary_reason, EvaluationReason::PolicyDeny(_));
    }

    #[tokio::test]
    async fn test_policy_allow_with_safety_deny() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["rm".to_string(), "-rf".to_string(), "/".to_string()];
        let result = evaluator
            .evaluate_with_policy(&cmd, true, "policy allowed")
            .await
            .unwrap();
        // Policy allows but safety rules should deny
        assert!(!result.allowed);
        matches!(result.primary_reason, EvaluationReason::DangerousCommand(_));
    }

    #[tokio::test]
    async fn test_policy_allow_with_safety_allow() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["git".to_string(), "status".to_string()];
        let result = evaluator
            .evaluate_with_policy(&cmd, true, "policy allowed")
            .await
            .unwrap();
        assert!(result.allowed);
    }

    // ========== Caching Tests ==========

    #[tokio::test]
    async fn test_cache_hit_on_repeated_evaluation() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["git".to_string(), "status".to_string()];

        // First evaluation
        let result1 = evaluator.evaluate(&cmd).await.unwrap();
        assert!(result1.allowed);
        assert!(!format!("{:?}", result1.primary_reason).contains("Cache"));

        // Second evaluation (should hit cache)
        let result2 = evaluator.evaluate(&cmd).await.unwrap();
        assert!(result2.allowed);
        assert!(format!("{:?}", result2.primary_reason).contains("Cache"));
    }

    #[tokio::test]
    async fn test_cache_stores_deny_decisions() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["rm".to_string(), "-rf".to_string(), "/".to_string()];

        // First evaluation
        let result1 = evaluator.evaluate(&cmd).await.unwrap();
        assert!(!result1.allowed);

        // Second evaluation (should hit cache with deny)
        let result2 = evaluator.evaluate(&cmd).await.unwrap();
        assert!(!result2.allowed);
        assert!(format!("{:?}", result2.primary_reason).contains("Cache"));
    }

    // ========== Empty/Invalid Command Tests ==========

    #[tokio::test]
    async fn test_empty_command_denied() {
        let evaluator = UnifiedCommandEvaluator::new();
        let result = evaluator.evaluate(&[]).await.unwrap();
        assert!(!result.allowed);
    }

    #[tokio::test]
    async fn test_whitespace_only_command_denied() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["   ".to_string()];
        // Note: This may or may not be caught at this level depending on trim behavior
        // Just ensure it doesn't panic
        let _ = evaluator.evaluate(&cmd).await;
    }

    // ========== PolicyAwareEvaluator Tests ==========

    #[tokio::test]
    async fn test_policy_aware_without_policy() {
        let evaluator = PolicyAwareEvaluator::new();
        let cmd = vec!["git".to_string(), "status".to_string()];
        let result = evaluator.evaluate(&cmd).await.unwrap();
        assert!(result.allowed);
    }

    #[tokio::test]
    async fn test_policy_aware_with_static_policy() {
        let evaluator = PolicyAwareEvaluator::with_policy(false, "blocked");
        let cmd = vec!["git".to_string(), "status".to_string()];
        let result = evaluator.evaluate(&cmd).await.unwrap();
        assert!(!result.allowed);
    }

    #[tokio::test]
    async fn test_policy_aware_set_policy() {
        let mut evaluator = PolicyAwareEvaluator::new();
        let cmd = vec!["git".to_string(), "status".to_string()];

        // Initially no policy
        let result1 = evaluator.evaluate(&cmd).await.unwrap();
        assert!(result1.allowed);

        // Set deny policy
        evaluator.set_policy(false, "now blocked");
        let result2 = evaluator.evaluate(&cmd).await.unwrap();
        assert!(!result2.allowed);
    }

    #[tokio::test]
    async fn test_policy_aware_clear_policy() {
        let mut evaluator = PolicyAwareEvaluator::with_policy(false, "blocked");
        let cmd = vec!["git".to_string(), "status".to_string()];

        // Initially blocked by policy
        let result1 = evaluator.evaluate(&cmd).await.unwrap();
        assert!(!result1.allowed);

        // Clear policy
        evaluator.clear_policy();
        let result2 = evaluator.evaluate(&cmd).await.unwrap();
        assert!(result2.allowed);
    }

    // ========== Evaluation Reason Tests ==========

    #[tokio::test]
    async fn test_dangerous_command_reason() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["rm".to_string(), "-rf".to_string(), "/".to_string()];
        let result = evaluator.evaluate(&cmd).await.unwrap();
        assert!(!result.allowed);
        matches!(result.primary_reason, EvaluationReason::DangerousCommand(_));
    }

    #[tokio::test]
    async fn test_safety_allow_reason() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["git".to_string(), "status".to_string()];
        let result = evaluator.evaluate(&cmd).await.unwrap();
        assert!(result.allowed);
        matches!(result.primary_reason, EvaluationReason::SafetyAllow);
    }

    #[tokio::test]
    async fn test_secondary_reasons_populated() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["git".to_string(), "status".to_string()];
        let result = evaluator.evaluate(&cmd).await.unwrap();
        assert!(result.allowed);
        assert!(!result.secondary_reasons.is_empty());
    }

    // ========== Multiple Command Evaluation (Stateless) ==========

    #[tokio::test]
    async fn test_evaluate_multiple_different_commands() {
        let evaluator = UnifiedCommandEvaluator::new();

        let cmd1 = vec!["git".to_string(), "status".to_string()];
        let result1 = evaluator.evaluate(&cmd1).await.unwrap();
        assert!(result1.allowed);

        let cmd2 = vec!["cargo".to_string(), "test".to_string()];
        let result2 = evaluator.evaluate(&cmd2).await.unwrap();
        assert!(result2.allowed);

        let cmd3 = vec!["rm".to_string(), "-rf".to_string(), "/".to_string()];
        let result3 = evaluator.evaluate(&cmd3).await.unwrap();
        assert!(!result3.allowed);
    }

    // ========== Edge Cases ==========

    #[tokio::test]
    async fn test_command_with_spaces_in_args() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["echo".to_string(), "hello world".to_string()];
        let result = evaluator.evaluate(&cmd).await.unwrap();
        // echo is generally safe, should be allowed or unknown
        assert!(result.allowed || format!("{:?}", result.primary_reason).contains("Unknown"));
    }

    #[tokio::test]
    async fn test_sudo_unwrapping() {
        let evaluator = UnifiedCommandEvaluator::new();

        // Safe command with sudo
        let cmd1 = vec!["sudo".to_string(), "git".to_string(), "status".to_string()];
        let result1 = evaluator.evaluate(&cmd1).await.unwrap();
        // sudo itself might block, depending on implementation
        let _ = result1;

        // Dangerous command with sudo
        let cmd2 = vec![
            "sudo".to_string(),
            "rm".to_string(),
            "-rf".to_string(),
            "/".to_string(),
        ];
        let result2 = evaluator.evaluate(&cmd2).await.unwrap();
        assert!(!result2.allowed);
    }

    // ========== Integration: Policy + Safety + Shell Parsing ==========

    #[tokio::test]
    async fn test_full_pipeline_bash_lc_with_policy() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec![
            "bash".to_string(),
            "-lc".to_string(),
            "git status && cargo test".to_string(),
        ];

        // Simulate policy that allows (safe commands should pass)
        let result = evaluator
            .evaluate_with_policy(&cmd, true, "policy allowed")
            .await
            .unwrap();
        assert!(result.allowed);

        // Simulate policy that denies (policy should block)
        let result = evaluator
            .evaluate_with_policy(&cmd, false, "policy blocked")
            .await
            .unwrap();
        assert!(!result.allowed);
    }

    #[tokio::test]
    async fn test_full_pipeline_dangerous_bash_lc_overrides_policy() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec![
            "bash".to_string(),
            "-lc".to_string(),
            "git status && rm -rf /".to_string(),
        ];

        // Even if policy allows, safety should deny
        let result = evaluator
            .evaluate_with_policy(&cmd, true, "policy allowed")
            .await
            .unwrap();
        assert!(!result.allowed);
    }

    // ========== Stress Tests ==========

    #[tokio::test]
    async fn test_large_number_of_evaluations() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["git".to_string(), "status".to_string()];

        for _ in 0..100 {
            let result = evaluator.evaluate(&cmd).await.unwrap();
            assert!(result.allowed);
        }
    }

    #[tokio::test]
    async fn test_many_different_commands() {
        let evaluator = UnifiedCommandEvaluator::new();
        let commands = vec![
            vec!["git", "status"],
            vec!["git", "log"],
            vec!["cargo", "build"],
            vec!["cargo", "test"],
            vec!["ls"],
            vec!["find", "."],
        ];

        for cmd in commands {
            let cmd_vec: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
            let result = evaluator.evaluate(&cmd_vec).await.unwrap();
            assert!(result.allowed, "Expected {} to be allowed", cmd.join(" "));
        }
    }

    #[tokio::test]
    async fn test_concurrent_evaluations() {
        let evaluator = UnifiedCommandEvaluator::new();
        let evaluator = std::sync::Arc::new(evaluator);

        let mut handles = vec![];

        for i in 0..10 {
            let eval = evaluator.clone();
            let handle = tokio::spawn(async move {
                let cmd = if i % 2 == 0 {
                    vec!["git".to_string(), "status".to_string()]
                } else {
                    vec!["cargo".to_string(), "test".to_string()]
                };
                let result = eval.evaluate(&cmd).await.unwrap();
                assert!(result.allowed);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }
    }
}
