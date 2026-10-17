use super::*;
use serde_json::json;

#[test]
fn iter_keys_suppresses_duplicate_pattern_key() {
    let target = ApprovalLearningTarget::new("same".into(), "exact-label".into())
        .with_pattern(Some(LearnedPattern { key: "same".into(), label: "pattern-label".into() }));
    assert_eq!(target.iter_keys().collect::<Vec<_>>(), vec![("same", "exact-label")]);
}

#[test]
fn web_fetch_domain_targets_match_cache_keys() {
    for tool_name in ["web_fetch", "fetch_url"] {
        for url in ["https://Example.COM/path", "https://example.com/other"] {
            let args = json!({ "url": url });
            let expected_key = format!("{tool_name}:example.com");
            let learning = approval_learning_target(tool_name, Some(&args), "fallback");
            let exact = exact_shell_approval_target(tool_name, Some(&args), "fallback").expect("domain target");
            assert_eq!(super::super::approval_cache::cache_key(tool_name, Some(&args)), expected_key);
            assert_eq!(learning.approval_key, expected_key);
            assert_eq!(exact.approval_key, expected_key);
            assert_eq!(learning.display_label, "fetch from example.com");
            assert_eq!(exact.display_label, learning.display_label);
            assert!(learning.pattern.is_none());
            assert!(exact.pattern.is_none());
            assert!(matches!(
                persistent_approval_target(tool_name, Some(&args), "fallback"),
                PersistentApprovalTarget::ExactInvocation { display_label } if display_label == learning.display_label
            ));
        }
    }
}

#[test]
fn malformed_web_fetch_urls_keep_tool_level_fallback() {
    for tool_name in ["web_fetch", "fetch_url"] {
        for args in [
            json!({"url": "not a URL"}),
            json!({"url": "https://"}),
            json!({"url": "file:///tmp/file"}),
            json!({"url": 42}),
            json!({}),
        ] {
            let learning = approval_learning_target(tool_name, Some(&args), "fallback");
            assert_eq!(learning.approval_key, tool_name);
            assert_eq!(learning.display_label, "fallback");
            assert_eq!(super::super::approval_cache::cache_key(tool_name, Some(&args)), tool_name);
            assert!(exact_shell_approval_target(tool_name, Some(&args), "fallback").is_none());
            assert!(matches!(
                persistent_approval_target(tool_name, Some(&args), "fallback"),
                PersistentApprovalTarget::ToolLevel
            ));
        }
    }
}

#[test]
fn compound_patterns_sort_and_deduplicate_families() {
    let scope = "sandbox_permissions=\"use_default\"|additional_permissions=null";
    for command in ["wc -l src/a.rs; ls docs; ls src", "ls src; wc -l src/a.rs; ls docs"] {
        let args = json!({"action": "run", "command": command});
        let target = exact_shell_learning_target("exec_command", Some(&args), "fallback").expect("target");
        assert_eq!(target.approval_key, format!("shell-pattern:ls|{scope}&&shell-pattern:wc|{scope}"));
        assert_eq!(target.display_label, "safe `ls` path reads and safe `wc` path reads");
    }
}

#[test]
fn approval_keys_separate_permission_scopes() {
    for command in ["find src -type f", "ls src; wc -l src/a.rs"] {
        let base = json!({"action": "run", "command": command});
        let escalated = json!({"action": "run", "command": command, "sandbox_permissions": "require_escalated"});
        let additional = json!({"action": "run", "command": command, "additional_permissions": {"fs_write": ["/tmp/approval-scope"]}});
        let targets = [&base, &escalated, &additional]
            .map(|args| approval_learning_target("exec_command", Some(args), "fallback"));
        for left in 0..targets.len() {
            for right in left + 1..targets.len() {
                for (left_key, _) in targets[left].iter_keys() {
                    for (right_key, _) in targets[right].iter_keys() {
                        assert_ne!(left_key, right_key, "permission scopes must not share approvals: {command}");
                    }
                }
            }
        }
    }
}

fn pattern_for(command: &str) -> Option<LearnedPattern> {
    let args = json!({ "action": "run", "command": command });
    learned_shell_pattern("exec_command", Some(&args))
}

#[test]
fn find_under_subdir_yields_pattern_key() {
    let pattern = pattern_for("find src -type f -name '*.rs'").expect("pattern");
    assert!(pattern.key.starts_with("shell-pattern:find src|sandbox_permissions="));
    assert_eq!(pattern.label, "safe `find src` commands");
}

#[test]
fn find_root_directory_does_not_get_pattern() {
    assert!(pattern_for("find . -type f").is_none());
    assert!(pattern_for("find / -type f").is_none());
    assert!(pattern_for("find ./ -type f").is_none());
}

#[test]
fn find_with_destructive_flags_does_not_get_pattern() {
    assert!(pattern_for("find src -delete").is_none());
    assert!(pattern_for("find src -exec rm {} +").is_none());
    assert!(pattern_for("find src -name foo -ok rm {} \\;").is_none());
}

#[test]
fn find_with_spliced_destructive_flag_does_not_get_pattern() {
    for command in [
        "find src -maxdepth 0 -exe$''c touch /tmp/VT_BYPASS_POC {} +",
        "find src -maxdepth 0 -exe$@c touch /tmp/VT_BYPASS_POC {} +",
        "find src -maxdepth 0 -exe$*c touch /tmp/VT_BYPASS_POC {} +",
        "find src -maxdepth 0 -ex{e,}c touch /tmp/VT_BYPASS_POC {} +",
        "find src -maxdepth 0 -ex* touch /tmp/VT_BYPASS_POC {} +",
    ] {
        assert!(pattern_for(command).is_none(), "dynamic find syntax must not learn: {command}");
    }
}

#[test]
fn path_qualified_programs_stay_exact_only() {
    // A path-qualified executable (`./find`, `/usr/bin/find`, `bin/find`)
    // can be an agent-created binary, so it must not inherit the bare
    // `find` family approval, and must not fall through to a generic
    // `shell-pattern:/usr/bin/find` family either.
    for command in [
        "/usr/bin/find src -type f -name '*.rs'",
        "./find src -type f",
        "bin/find src -type f",
        "/usr/bin/FIND src -type f",
        "/usr/bin/find src -delete",
        "/usr/bin/find src -maxdepth 0 -exe$''c touch /tmp/VT_BYPASS_POC {} +",
        "/usr/bin/find src -maxdepth 0 -ex'ec' touch /tmp/VT_BYPASS_POC {} +",
        "/bin/find src -del'ete'",
    ] {
        assert!(pattern_for(command).is_none(), "path-qualified program must stay exact-only: {command}");
    }
    // The bare program still learns.
    assert!(pattern_for("find src -type f -name '*.rs'").is_some());
}

#[test]
fn wrapper_and_env_prefixes_do_not_learn_families() {
    // Wrappers and environment/assignment prefixes can reselect the
    // executable or change the effective directory, so they never train
    // (or inherit) a family key — including the CodeRabbit-reported
    // `env PATH=./bin find` and `env -C <dir> sed` shapes.
    for command in [
        "env find src -type f",
        "FOO=bar find src -type f",
        "PATH=./bin find src -type f",
        "env PATH=./bin find src -type f",
        "env -C /tmp sed -n '1p' src/file.rs",
        "env find src -maxdepth 0 -exe$''c touch /tmp/VT_BYPASS_POC {} +",
        "FOO=bar find src -delete",
        "sudo find src -type f",
        "nice find src -type f",
        "env sudo find src -type f",
        "FOO=bar grep -r foo src",
        "FOO=bar rm -rf target",
        "sudo ls src",
    ] {
        assert!(pattern_for(command).is_none(), "wrapper/env must stay exact-only: {command}");
    }
}

#[test]
fn compound_with_unsafe_segment_does_not_learn() {
    // A safe segment must not supply a family key that would let
    // `prompt_tool_permission` auto-approve a sibling unsafe segment.
    for command in [
        "ls src && rm foo.txt",
        "ls src; python3 mutate.py",
        "ls src && PATH=./bin find src -type f",
        "cat docs/a.md && sed -i 's/a/b/' src/lib.rs",
    ] {
        let args = json!({ "action": "run", "command": command });
        let key = exact_shell_learning_target("exec_command", Some(&args), "Run Command")
            .expect("exact target")
            .approval_key;
        assert!(
            !key.starts_with("shell-pattern:"),
            "compound with an unsafe segment must not learn a family key: {command} -> {key}"
        );
    }
}

#[test]
fn generic_pattern_rejects_dynamic_shell_syntax() {
    assert!(pattern_for("ls src").is_some());
    for command in ["ls src/$FOO", "grep -r foo src/*.rs", "wc -l src/file.txt; echo hi"] {
        // `;` is already rejected by the prefix gate; `$`/glob shapes must
        // fail closed via the new dynamic-syntax gate.
        if command.contains(';') {
            assert!(pattern_for(command).is_none());
        } else {
            assert!(pattern_for(command).is_none(), "dynamic generic must not learn: {command}");
        }
    }
}

#[test]
fn mixed_case_and_spliced_programs_stay_exact_only() {
    // Only the bare lowercase program trains a family; uppercase or
    // quoted-spliced spellings stay exact-only (and cannot dodge the
    // find-specific rules via the generic fallback).
    assert!(pattern_for("FIND src -type f").is_none());
    assert!(pattern_for("/usr/bin/FIND src -delete").is_none());
    assert!(pattern_for("find src -del'ete'").is_none());
    assert!(pattern_for("/usr/bin/find src -del'ete'").is_none());
}

#[test]
fn compound_shell_commands_do_not_get_pattern() {
    assert!(pattern_for("find src -type f ; rm -rf target").is_none());
    assert!(pattern_for("find src -type f && rm -rf target").is_none());
    assert!(pattern_for("find src -type f || true").is_none());
    assert!(pattern_for("find src -type f | xargs rm").is_none());
    assert!(pattern_for("bash -c 'find src -type f'").is_none());
    assert!(pattern_for("sh -lc \"find src -type f\"").is_none());
}

#[test]
fn absolute_and_traversal_roots_do_not_get_pattern() {
    assert!(pattern_for("find /tmp -type f").is_none());
    assert!(pattern_for("find /Users/me/project -type f").is_none());
    assert!(pattern_for("find ../other -type f").is_none());
    assert!(pattern_for("find src/../other -type f").is_none());
    assert!(pattern_for("find ~/src -type f").is_none());
    assert!(pattern_for("find ~ -type f").is_none());
    assert!(pattern_for("find / -type f").is_none());
}

#[test]
fn mutating_commands_have_no_pattern() {
    assert!(pattern_for("rm -rf target").is_none());
    assert!(pattern_for("cp src/lib.rs /tmp/lib.rs").is_none());
    assert!(pattern_for("mkdir build").is_none());
}

#[test]
fn readonly_path_commands_get_generic_pattern() {
    let pattern = pattern_for("grep -r foo src").expect("pattern");
    assert!(pattern.key.starts_with("shell-pattern:grep|"));

    let pattern = pattern_for("ls src").expect("pattern");
    assert!(pattern.key.starts_with("shell-pattern:ls|"));

    let pattern = pattern_for("wc -l src/main.rs").expect("pattern");
    assert!(pattern.key.starts_with("shell-pattern:wc|"));
}

#[test]
fn sed_print_under_workspace_path_yields_pattern_key() {
    let pattern =
        pattern_for("sed -n '87,140p' crates/codegen/vtcode-core/src/core/agent/features.rs").expect("pattern");

    assert!(
        pattern
            .key
            .starts_with("shell-pattern:sed -n <range> crates|sandbox_permissions=")
    );
    assert_eq!(pattern.label, "safe `sed -n` reads under `crates`");
}

#[test]
fn sed_without_print_range_has_no_pattern() {
    assert!(pattern_for("sed -i 's/a/b/' src/lib.rs").is_none());
    assert!(pattern_for("sed -n '1,10d' src/lib.rs").is_none());
    assert!(pattern_for("sed -n '1,10p' ../src/lib.rs").is_none());
}

#[test]
fn awk_range_print_under_workspace_path_yields_pattern_key() {
    let pattern =
        pattern_for("awk 'NR>=895 && NR<=935 {print NR\": \"$0}' src/agent/runloop/orchestration.rs").expect("pattern");

    assert!(pattern.key.starts_with("shell-pattern:awk src|sandbox_permissions="));
    assert_eq!(pattern.label, "safe `awk` reads under `src`");
}

#[test]
fn awk_with_data_options_yields_same_family_key() {
    let plain = pattern_for("awk 'NR>=40 && NR<=140' README.md").expect("pattern");
    let field_sep = pattern_for("awk -F: '{print $1}' README.md").expect("pattern");
    let var_assign = pattern_for("awk -v limit=10 'NR<=limit' README.md").expect("pattern");

    for pattern in [&plain, &field_sep, &var_assign] {
        assert!(pattern.key.starts_with("shell-pattern:awk README.md|sandbox_permissions="));
    }
    assert_eq!(plain.key.split('|').next(), field_sep.key.split('|').next());
    assert_eq!(plain.key.split('|').next(), var_assign.key.split('|').next());
}

#[test]
fn awk_read_with_quoted_pipe_yields_family_pattern() {
    // The reported shape: the quoted `"|"` argument to `index()` previously
    // tripped the naive control-operator gate, so no family key was
    // attached and every new `NR` range re-prompted. It must now learn.
    let pattern = pattern_for(r#"awk 'NR>=208 && NR<=212 {n=index(rest,"|"); print n}' README.md"#)
        .expect("quoted-pipe awk read must yield a family pattern");
    assert!(pattern.key.starts_with("shell-pattern:awk README.md|sandbox_permissions="));
    assert_eq!(pattern.label, "safe `awk` reads under `README.md`");

    // Exact reported multi-line program (single-quoted, so `$0`/`\$` are
    // literal awk text, not shell expansion) must also learn.
    let reported = r#"awk 'NR>=208 && NR<=212 {line=$0; body=substr(line,1,length(line)-1); n=0; while (body ~ / \$/) { body=substr(body,1,length(body)-1); n++ }} # find guide start after label cell
rest=substr(line,3); g=index(rest,"|")+1; guide=substr(rest,g+2); gp=0; gg=guide; while (gg ~ / \$/) { gg=substr(gg,1,length(gg)-1); gp++ } print "%d: linelen=%d labelcell=%s pad_before_final_pipe=%d guide_pad=%d\n", NR, length(line), substr(line,3,20), n, gp }' README.md"#;
    let reported_pattern = pattern_for(reported).expect("reported multi-line awk read must yield a family pattern");
    assert!(
        reported_pattern
            .key
            .starts_with("shell-pattern:awk README.md|sandbox_permissions=")
    );
}

#[test]
fn safe_awk_read_offers_learned_family_permanent_target() {
    // "Always approve" must scope to the safe learned family, not the exact
    // line range, so a new `awk 'NR>=a && NR<=b' README.md` does not re-prompt.
    for command in [
        r#"awk 'NR>=208 && NR<=212 {n=index(rest,"| "); print n}' README.md"#,
        "awk 'NR>=1 && NR<=5' README.md",
        "awk -F: '{print $1}' README.md",
        "awk '$3>100' README.md",
        "awk '{if(p2==0 && i>1) print}' README.md",
    ] {
        let args = json!({"action": "run", "command": command});
        match persistent_approval_target("exec_command", Some(&args), "Run Command") {
            PersistentApprovalTarget::LearnedPattern { key, display_label } => {
                assert!(key.starts_with("shell-pattern:awk README.md|sandbox_permissions="), "got {key}");
                assert_eq!(display_label, "safe `awk` reads under `README.md`");
            }
            other => panic!("expected learned family target for {command}, got {other:?}"),
        }
    }
}

#[test]
fn mutating_awk_keeps_exact_permanent_target() {
    let args = json!({"action": "run", "command": "awk '{print > \"out.txt\"}' README.md"});
    assert!(matches!(
        persistent_approval_target("exec_command", Some(&args), "Run Command"),
        PersistentApprovalTarget::ExactInvocation { .. }
    ));
}

#[test]
fn awk_mutating_shapes_have_no_pattern() {
    for command in [
        "awk '{print > \"out.txt\"}' README.md",
        "awk '{print >> \"out.txt\"}' README.md",
        "awk '{print | \"sort\"}' README.md",
        "awk 'BEGIN{system(\"touch out\")}' README.md",
        "awk -i inplace '{print}' README.md",
        "awk -f program.awk README.md",
        "awk '@include \"x.awk\"' README.md",
        "awk '@load \"ext\"' README.md",
        "awk -v f=system 'BEGIN{@f(\"id\")}' README.md",
        "awk 'BEGIN{@s(\"id\")}' README.md",
        "awk -l injail '{print}' README.md",
        "awk 'BEGIN { # (\n print \"written\" > \"out.txt\" }' README.md",
        "awk 'BEGIN { # [\n printf \"written\" > \"out.txt\" }' README.md",
        "awk '/error|warning/' README.md",
        "awk 'NR>=1' README.md > out.txt",
        "awk -F:",
        "awk",
    ] {
        assert!(pattern_for(command).is_none(), "mutating awk must not learn: {command}");
    }
}

#[test]
fn awk_dynamic_syntax_has_no_pattern() {
    for command in [
        "awk 'NR>=1' src/file.txt $(whoami)",
        "awk 'NR>=1' src/file.txt `whoami`",
        "awk 'NR>=1' src/*.rs",
        "awk 'NR>=1' src/file.txt; echo hi",
    ] {
        assert!(pattern_for(command).is_none(), "dynamic awk must not learn: {command}");
    }
}

#[test]
fn awk_multi_family_and_stdin_have_no_pattern() {
    assert!(pattern_for("awk 'NR>=1' src/a.rs docs/b.md").is_none());
    assert!(pattern_for("awk 'NR>=1' src/a.rs src/../other/b.rs").is_none());
    assert!(pattern_for("awk '{print $1}' -").is_none());
    assert!(pattern_for("awk 'NR>=1'").is_none());
}

#[test]
fn awk_absolute_and_traversal_paths_have_no_pattern() {
    assert!(pattern_for("awk 'NR>=1 && NR<=5' /tmp/file.txt").is_none());
    assert!(pattern_for("awk 'NR>=1 && NR<=5' ../src/lib.rs").is_none());
    assert!(pattern_for("awk 'NR>=1 && NR<=5' src/../other/file.txt").is_none());
}

#[test]
fn awk_compound_commands_have_no_pattern() {
    assert!(pattern_for("awk 'NR>=1' src/file.txt && rm -rf target").is_none());
    assert!(pattern_for("awk 'NR>=1' src/file.txt | xargs rm").is_none());
}

#[test]
fn ls_multiple_absolute_paths_yields_compact_pattern_key() {
    let pattern = pattern_for(
        "ls /Users/me/project/.claude/agents/ /Users/me/project/.codex/agents/ /Users/me/project/.opencode/agents/",
    )
    .expect("pattern");

    assert!(pattern.key.starts_with("shell-pattern:ls|sandbox_permissions="));
    assert_eq!(pattern.label, "safe `ls` path reads");
}

#[test]
fn compound_ls_commands_use_compact_segmented_target() {
    let args = json!({
        "action": "run",
        "command": "ls /Users/me/project/.agents/ 2>/dev/null; ls /Users/me/project/docs/ 2>/dev/null"
    });
    let target = exact_shell_learning_target("exec_command", Some(&args), "Run Command").expect("target");

    assert_eq!(
        target.approval_key,
        "shell-pattern:ls|sandbox_permissions=\"use_default\"|additional_permissions=null"
    );
    assert_eq!(target.display_label, "safe `ls` path reads");
}

#[test]
fn compound_with_a_patternless_segment_stays_exact_only() {
    // CodeRabbit: a safe segment must not supply a lone key for a
    // path-qualified `./find` segment (agent-created binary), a bare
    // find/sed/awk segment, or any unsafe segment.
    for command in [
        "ls src && ./find src -type f",
        "ls src && find src -type f",
        "ls src; python3 mutate.py",
    ] {
        let args = json!({ "action": "run", "command": command });
        let key = exact_shell_learning_target("exec_command", Some(&args), "Run Command")
            .expect("exact target")
            .approval_key;
        assert!(
            !key.starts_with("shell-pattern:"),
            "compound with a pattern-less/unsafe segment must stay exact-only: {command} -> {key}"
        );
    }
}

#[test]
fn unknown_non_mutating_path_command_gets_compact_pattern() {
    let pattern = pattern_for("wc -l src/lib.rs README.md").expect("pattern");
    assert!(pattern.key.starts_with("shell-pattern:wc|"));
    assert_eq!(pattern.label, "safe `wc` path reads");
}

#[test]
fn mutating_path_commands_do_not_get_generic_pattern() {
    assert!(pattern_for("rm src/lib.rs").is_none());
    assert!(pattern_for("cp src/lib.rs /tmp/lib.rs").is_none());
    assert!(pattern_for("perl -i -pe 's/a/b/' src/lib.rs").is_none());
}

#[test]
fn find_subdir_path_collapses_to_first_segment() {
    let pattern = pattern_for("find src/agent/runloop -type f").expect("pattern");
    assert!(pattern.key.starts_with("shell-pattern:find src|sandbox_permissions="));
}

#[test]
fn iter_keys_yields_only_exact_when_no_pattern() {
    let target = ApprovalLearningTarget::new("key".into(), "label".into());
    let keys: Vec<_> = target.iter_keys().collect();
    assert_eq!(keys, vec![("key", "label")]);
}

#[test]
fn iter_keys_yields_pattern_after_exact_when_present() {
    let target = ApprovalLearningTarget::new("exact".into(), "exact-label".into()).with_pattern(Some(LearnedPattern {
        key: "pattern".into(),
        label: "pattern-label".into(),
    }));
    let keys: Vec<_> = target.iter_keys().collect();
    assert_eq!(keys, vec![("exact", "exact-label"), ("pattern", "pattern-label")]);
}

#[tokio::test]
async fn record_blocking_records_both_exact_and_pattern_keys() {
    use vtcode_core::tools::ApprovalRecorder;

    let temp_dir = std::env::temp_dir().join(format!(
        "vtcode_record_blocking_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let recorder = ApprovalRecorder::new(temp_dir.clone());
    let target = approval_learning_target(
        "exec_command",
        Some(&json!({"action":"run","command":"find src -type f"})),
        "default",
    );
    let pattern = target.pattern.as_ref().expect("pattern attached");

    super::super::approval_cache::record_approval_blocking(&recorder, &target, true).await;

    assert_eq!(recorder.get_approval_count(&target.approval_key).await, 1);
    assert_eq!(recorder.get_approval_count(&pattern.key).await, 1);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn denial_propagates_to_pattern_key() {
    use vtcode_core::tools::ApprovalRecorder;

    let temp_dir = std::env::temp_dir().join(format!(
        "vtcode_pattern_denial_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let recorder = ApprovalRecorder::new(temp_dir.clone());
    let target = approval_learning_target(
        "exec_command",
        Some(&json!({"action":"run","command":"find src -type f"})),
        "default",
    );
    let pattern = target.pattern.as_ref().expect("pattern attached");

    super::super::approval_cache::record_approval_blocking(&recorder, &target, false).await;

    assert_eq!(recorder.get_approval_count(&target.approval_key).await, 0);
    assert_eq!(recorder.get_approval_count(&pattern.key).await, 0);
    // ...but the pattern key's deny_count is bumped, so a future burst of
    // approvals is tempered when computing approval rate.
    let stored = recorder.get_pattern(&pattern.key).await.expect("stored");
    assert_eq!(stored.deny_count, 1);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn three_safe_find_invocations_promote_pattern_to_auto_approve() {
    use vtcode_core::tools::ApprovalRecorder;

    let temp_dir = std::env::temp_dir().join(format!(
        "vtcode_pattern_promote_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let recorder = ApprovalRecorder::new(temp_dir.clone());

    // Three different (but equally safe) `find src ...` approvals,
    // simulating the user manually approving each variant.
    for command in [
        "find src -type f -name '*.rs'",
        "find src -type d",
        "find src -name foo",
    ] {
        let target =
            approval_learning_target("exec_command", Some(&json!({"action":"run","command":command})), "default");
        super::super::approval_cache::record_approval_blocking(&recorder, &target, true).await;
    }

    // A *new* safe `find src ...` invocation should auto-approve via the
    // pattern key even though its exact form has never been seen before.
    let new_target = approval_learning_target(
        "exec_command",
        Some(&json!({"action":"run","command":"find src -path '*runloop*'"})),
        "default",
    );
    let pattern = new_target.pattern.as_ref().expect("pattern attached");
    assert!(recorder.should_auto_approve(&pattern.key).await);
    assert_eq!(recorder.get_approval_count(&new_target.approval_key).await, 0);

    // Destructive `find src -delete` MUST NOT inherit the pattern.
    let destructive = approval_learning_target(
        "exec_command",
        Some(&json!({"action":"run","command":"find src -delete"})),
        "default",
    );
    assert!(destructive.pattern.is_none(), "destructive find must not carry pattern");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn three_safe_awk_invocations_promote_pattern_to_auto_approve() {
    use vtcode_core::tools::ApprovalRecorder;

    let temp_dir = std::env::temp_dir().join(format!(
        "vtcode_awk_pattern_promote_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let recorder = ApprovalRecorder::new(temp_dir.clone());

    // Three different (but equally safe) `awk ... src/...` approvals with
    // distinct `NR` ranges — the exact shape that previously re-prompted.
    for command in [
        "awk 'NR>=895 && NR<=935 {print NR\": \"$0}' src/agent/runloop/orchestration.rs",
        "awk 'NR>=40 && NR<=140' src/lib.rs",
        "awk -F: '{print $1}' src/main.rs",
    ] {
        let target =
            approval_learning_target("exec_command", Some(&json!({"action":"run","command":command})), "default");
        super::super::approval_cache::record_approval_blocking(&recorder, &target, true).await;
    }

    // A *new* safe `awk ... src/...` range should auto-approve via the
    // family key even though its exact form was never seen before.
    let new_target = approval_learning_target(
        "exec_command",
        Some(&json!({"action":"run","command":"awk 'NR>=1 && NR<=5' src/other.rs"})),
        "default",
    );
    let pattern = new_target.pattern.as_ref().expect("pattern attached");
    assert!(recorder.should_auto_approve(&pattern.key).await);
    assert_eq!(recorder.get_approval_count(&new_target.approval_key).await, 0);

    // Mutating `awk` MUST NOT inherit the family pattern.
    let destructive = approval_learning_target(
        "exec_command",
        Some(&json!({"action":"run","command":"awk '{print > \"out.txt\"}' src/other.rs"})),
        "default",
    );
    assert!(destructive.pattern.is_none(), "destructive awk must not carry pattern");

    let _ = std::fs::remove_dir_all(&temp_dir);
}
