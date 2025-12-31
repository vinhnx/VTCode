//! Comprehensive tests for Phase 2: Enhanced command safety features
//!
//! Tests for:
//! - Command database
//! - Audit logging
//! - Performance caching
//! - Integration scenarios

#[cfg(test)]
mod integration_tests {
    use crate::command_safety::{
        SafeCommandRegistry, SafetyDecisionCache, SafetyAuditLogger, AuditEntry, SafetyDecision,
    };

    #[test]
    fn registry_and_cache_integration() {
        let registry = SafeCommandRegistry::new();
        let cache = SafetyDecisionCache::new(100);

        let cmd = vec!["git".to_string(), "status".to_string()];

        // First check: cache miss
        let cmd_str = cmd.join(" ");
        let cached = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(cache.get(&cmd_str));
        assert!(cached.is_none());

        // Check registry
        let is_safe = registry.is_safe(&cmd);
        assert_eq!(is_safe, SafetyDecision::Allow);

        // Store in cache
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(cache.put(cmd_str.clone(), true, "git status allowed".to_string()));

        // Second check: cache hit
        let cached = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(cache.get(&cmd_str));
        assert!(cached.is_some());
        assert!(cached.unwrap().is_safe);
    }

    #[test]
    fn audit_logging_tracks_decisions() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let logger = SafetyAuditLogger::new(true);
        let registry = SafeCommandRegistry::new();

        rt.block_on(async {
            let cmd = vec!["git".to_string(), "status".to_string()];
            let decision = registry.is_safe(&cmd);

            // Log the decision
            let entry = AuditEntry::new(
                cmd.clone(),
                matches!(decision, SafetyDecision::Allow),
                "testing".to_string(),
                format!("{:?}", decision),
            );

            logger.log(entry).await;

            // Verify audit trail
            let entries = logger.entries().await;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].command, cmd);
        });
    }

    #[test]
    fn dangerous_vs_safe_commands_audit_trail() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let logger = SafetyAuditLogger::new(true);

        rt.block_on(async {
            // Safe command
            let safe_cmd = vec!["git".to_string(), "status".to_string()];
            logger
                .log(AuditEntry::new(
                    safe_cmd,
                    true,
                    "allowed".to_string(),
                    "Allow".to_string(),
                ))
                .await;

            // Dangerous command
            let dangerous_cmd = vec!["git".to_string(), "reset".to_string()];
            logger
                .log(AuditEntry::new(
                    dangerous_cmd,
                    false,
                    "denied".to_string(),
                    "Deny".to_string(),
                ))
                .await;

            // Check audit trail
            let all_entries = logger.entries().await;
            assert_eq!(all_entries.len(), 2);

            let denied = logger.denied_entries().await;
            assert_eq!(denied.len(), 1);
            assert!(!denied[0].allowed);
        });
    }

    #[test]
    fn cache_performance_scenario() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let cache = SafetyDecisionCache::new(100);
        let registry = SafeCommandRegistry::new();

        rt.block_on(async {
            let cmd = vec!["cargo".to_string(), "check".to_string()];
            let cmd_str = cmd.join(" ");

            // Evaluate same command 100 times
            for _ in 0..100 {
                if cache.get(&cmd_str).await.is_none() {
                    let is_safe = matches!(registry.is_safe(&cmd), SafetyDecision::Allow);
                    cache.put(cmd_str.clone(), is_safe, "cached".to_string()).await;
                }
            }

            // Verify cache effectiveness
            let stats = cache.stats().await;
            assert!(stats.total_accesses > 1, "Cache should have multiple accesses");
            assert_eq!(stats.entry_count, 1, "Should have 1 unique command");
        });
    }

    #[test]
    fn command_database_provides_rules() {
        use crate::command_safety::CommandDatabase;

        let rules = CommandDatabase::all_rules();
        assert!(rules.len() > 0, "Database should have rules");
        assert!(rules.contains_key("git"), "Should include git");
        assert!(rules.contains_key("cargo"), "Should include cargo");
        assert!(rules.contains_key("grep"), "Should include grep");
    }

    #[test]
    fn multi_command_safety_evaluation() {
        let registry = SafeCommandRegistry::new();

        let test_cases = vec![
            // (cmd, expected_safe, description)
            (vec!["git".to_string(), "status".to_string()], SafetyDecision::Allow, "git status"),
            (vec!["git".to_string(), "reset".to_string()], SafetyDecision::Deny(_), "git reset"),
            (vec!["cargo".to_string(), "check".to_string()], SafetyDecision::Allow, "cargo check"),
            (vec!["find".to_string(), ".".to_string()], SafetyDecision::Unknown, "find ."),
            (vec!["find".to_string(), ".".to_string(), "-delete".to_string()], SafetyDecision::Deny(_), "find -delete"),
        ];

        for (cmd, expected, desc) in test_cases {
            let result = registry.is_safe(&cmd);
            match expected {
                SafetyDecision::Allow => {
                    assert_eq!(result, SafetyDecision::Allow, "Failed for {}", desc);
                }
                SafetyDecision::Unknown => {
                    assert_eq!(result, SafetyDecision::Unknown, "Failed for {}", desc);
                }
                SafetyDecision::Deny(_) => {
                    assert!(matches!(result, SafetyDecision::Deny(_)), "Failed for {}", desc);
                }
            }
        }
    }
}

#[cfg(test)]
mod performance_tests {
    use crate::command_safety::SafetyDecisionCache;

    #[test]
    fn cache_eviction_under_load() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let cache = SafetyDecisionCache::new(10); // Small cache

        rt.block_on(async {
            // Insert 20 commands into a cache of size 10
            for i in 0..20 {
                let cmd = format!("cmd{}", i);
                cache.put(cmd, true, "allowed".to_string()).await;
            }

            // Cache should not exceed max size
            let size = cache.size().await;
            assert!(size <= 10, "Cache size {} should not exceed max 10", size);
        });
    }

    #[test]
    fn cache_lru_strategy() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let cache = SafetyDecisionCache::new(2);

        rt.block_on(async {
            // Insert 3 items (should evict the least used)
            cache.put("cmd1".to_string(), true, "allowed".to_string()).await;
            cache.put("cmd2".to_string(), true, "allowed".to_string()).await;

            // Access cmd1 multiple times
            let _d1 = cache.get("cmd1").await;
            let _d2 = cache.get("cmd1").await;
            let _d3 = cache.get("cmd1").await;

            // Insert cmd3 (should evict cmd2, not cmd1)
            cache.put("cmd3".to_string(), true, "allowed".to_string()).await;

            // cmd1 should still be there
            assert!(cache.get("cmd1").await.is_some());

            // cmd2 should be evicted (least used)
            assert!(cache.get("cmd2").await.is_none());
        });
    }
}

#[cfg(test)]
mod edge_case_tests {
    use crate::command_safety::{SafeCommandRegistry, SafetyDecision};

    #[test]
    fn command_with_absolute_path() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["/usr/bin/git".to_string(), "status".to_string()];
        let result = registry.is_safe(&cmd);
        assert_eq!(result, SafetyDecision::Allow);
    }

    #[test]
    fn command_with_multiple_options() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec![
            "find".to_string(),
            ".".to_string(),
            "-name".to_string(),
            "*.rs".to_string(),
            "-type".to_string(),
            "f".to_string(),
        ];
        let result = registry.is_safe(&cmd);
        assert_eq!(result, SafetyDecision::Unknown);
    }

    #[test]
    fn command_with_quoted_arguments() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec![
            "grep".to_string(),
            "\"search term\"".to_string(),
            "file.txt".to_string(),
        ];
        let result = registry.is_safe(&cmd);
        assert_eq!(result, SafetyDecision::Unknown);
    }

    #[test]
    fn empty_subcommand() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["git".to_string()];
        let result = registry.is_safe(&cmd);
        assert!(matches!(result, SafetyDecision::Deny(_)));
    }
}

#[cfg(test)]
mod real_world_scenarios {
    use crate::command_safety::{SafeCommandRegistry, SafetyDecision};

    #[test]
    fn typical_developer_workflow() {
        let registry = SafeCommandRegistry::new();

        // Typical day for a developer
        let commands = vec![
            // Git workflow
            (vec!["git".to_string(), "status".to_string()], SafetyDecision::Allow),
            (vec!["git".to_string(), "log".to_string(), "-10".to_string()], SafetyDecision::Allow),
            (vec!["git".to_string(), "diff".to_string(), "HEAD".to_string()], SafetyDecision::Allow),
            // Build
            (vec!["cargo".to_string(), "check".to_string()], SafetyDecision::Allow),
            (vec!["cargo".to_string(), "build".to_string()], SafetyDecision::Allow),
            // Search
            (vec!["grep".to_string(), "-r".to_string(), "TODO".to_string()], SafetyDecision::Unknown),
            // NOT allowed
            (vec!["git".to_string(), "reset".to_string(), "--hard".to_string()], SafetyDecision::Deny(_)),
        ];

        for (cmd, expected) in commands {
            let result = registry.is_safe(&cmd);
            match expected {
                SafetyDecision::Allow => assert_eq!(result, SafetyDecision::Allow),
                SafetyDecision::Unknown => assert_eq!(result, SafetyDecision::Unknown),
                SafetyDecision::Deny(_) => assert!(matches!(result, SafetyDecision::Deny(_))),
            }
        }
    }

    #[test]
    fn build_automation_scenario() {
        let registry = SafeCommandRegistry::new();

        // Automated CI/CD pipeline
        let commands = vec![
            (vec!["cargo".to_string(), "check".to_string()], SafetyDecision::Allow),
            (vec!["cargo".to_string(), "build".to_string()], SafetyDecision::Allow),
            (vec!["cargo".to_string(), "clippy".to_string()], SafetyDecision::Allow),
            (vec!["cargo".to_string(), "test".to_string()], SafetyDecision::Unknown),
        ];

        for (cmd, expected) in commands {
            let result = registry.is_safe(&cmd);
            match expected {
                SafetyDecision::Allow => assert_eq!(result, SafetyDecision::Allow),
                SafetyDecision::Unknown => assert_eq!(result, SafetyDecision::Unknown),
                SafetyDecision::Deny(_) => assert!(matches!(result, SafetyDecision::Deny(_))),
            }
        }
    }
}
