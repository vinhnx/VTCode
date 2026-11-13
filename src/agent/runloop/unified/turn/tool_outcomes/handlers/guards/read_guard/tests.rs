use vtcode_core::config::constants::tools as tool_names;

use super::*;

#[test]
fn repeated_file_read_family_key_tracks_cat_via_unified_exec() {
    let args = serde_json::json!({"command": "cat README.md"});
    let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(key, Some("unified_exec::run::cat README.md".to_string()));
}

#[test]
fn repeated_file_read_family_key_tracks_head_via_unified_exec() {
    let args = serde_json::json!({"command": "head -n 10 file.txt"});
    let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(key, Some("unified_exec::run::head -n 10 file.txt".to_string()));
}

#[test]
fn repeated_file_read_family_key_ignores_non_file_reading_commands() {
    let args = serde_json::json!({"command": "ls -la"});
    let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(key, None);
}

#[test]
fn repeated_file_read_family_key_ignores_git_status() {
    let args = serde_json::json!({"command": "git status"});
    let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(key, None);
}

#[test]
fn repeated_file_read_family_key_handles_cmd_alias() {
    let args = serde_json::json!({"cmd": "cat Cargo.toml"});
    let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(key, Some("unified_exec::run::cat Cargo.toml".to_string()));
}

#[test]
fn repeated_file_read_family_key_tracks_simple_sed_ranges_via_unified_exec() {
    let args = serde_json::json!({"command": "sed -n '440,520p' Cargo.toml"});
    let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(key, Some("unified_exec::read::Cargo.toml::off=440::lim=81".to_string()));
}

#[test]
fn repeated_file_read_family_key_tracks_simple_sed_ranges_via_public_exec_command() {
    let args = serde_json::json!({"cmd": "sed -n '440,520p' Cargo.toml"});
    let key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args);
    assert_eq!(key, Some("unified_exec::read::Cargo.toml::off=440::lim=81".to_string()));
}

#[test]
fn repeated_file_read_family_key_ignores_complex_sed_commands() {
    let args = serde_json::json!({"command": "sed -n '440,520p' Cargo.toml extra.txt"});
    let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(key, None);
}

#[test]
fn piped_sed_measurement_shares_family_with_bare_sed_same_range() {
    // Blocked session 20261008T094713Z: `sed -n '65,71p' README.md | awk ...`
    // measured the same table as the bare sed. Both must share one family
    // key so the consecutive family cap converges the loop.
    let bare = serde_json::json!({"cmd": "sed -n '65,71p' README.md"});
    let piped = serde_json::json!({"cmd": "sed -n '65,71p' README.md | awk '{n=length($0); print n}'"});
    let bare_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &bare);
    let piped_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &piped);
    assert_eq!(bare_key, Some("unified_exec::read::README.md::off=65::lim=7".to_string()));
    assert_eq!(piped_key, bare_key);
}

#[test]
fn piped_sed_different_ranges_stay_distinct_families() {
    // Asymmetric side: diverse pagination must NOT group. Same shape as
    // above but disjoint ranges → distinct keys, no false cap trip.
    let first = serde_json::json!({"cmd": "sed -n '65,71p' README.md | awk '{print n}'"});
    let second = serde_json::json!({"cmd": "sed -n '154,180p' README.md | awk '{print n}'"});
    let first_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &first);
    let second_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &second);
    assert_eq!(first_key, Some("unified_exec::read::README.md::off=65::lim=7".to_string()));
    assert_eq!(second_key, Some("unified_exec::read::README.md::off=154::lim=27".to_string()));
    assert_ne!(first_key, second_key);
}

#[test]
fn semicolon_compound_keeps_legacy_untracked_behavior() {
    // `;` compounds carry multiple reads in one call. They keep the legacy
    // `None` (under-count rather than over-block); pipe stages above are
    // the measurement-loop shape this fix covers.
    let args = serde_json::json!({"cmd": "sed -n '65,71p' README.md; echo ---; sed -n '202,260p' README.md"});
    assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args), None);
    assert_eq!(parse_simple_exec_read_target(&args), None);
}

#[test]
fn awk_nr_range_measurement_groups_same_slice() {
    // The late-turn loop ran `awk 'NR>=65 && NR<=71 ...' README.md` with
    // varying bodies. Same NR range → same family regardless of body.
    let first = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print NR\": \"$0}' README.md"});
    let second = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {n=length($0); print NR\": len=\"n}' README.md"});
    let first_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &first);
    let second_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &second);
    assert_eq!(first_key, Some("unified_exec::read::README.md::off=65::lim=7".to_string()));
    assert_eq!(second_key, first_key);
}

#[test]
fn awk_nr_equality_alternatives_extract_min_max() {
    // `NR==67 || NR==68 || ... || NR==71` targets lines 67-71.
    let args = serde_json::json!({"cmd": "awk 'NR==67 || NR==68 || NR==69 || NR==70 || NR==71 {print}' README.md"});
    let key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args);
    assert_eq!(key, Some("unified_exec::read::README.md::off=67::lim=5".to_string()));
}

#[test]
fn awk_whole_file_scan_stays_untracked() {
    // Asymmetric side: `length > 120` names a byte threshold, not a line
    // range (no NR) → untracked, so diverse whole-file queries never trip
    // the read caps (mirrors the `code_search` per-path exclusion).
    let whole = serde_json::json!({"cmd": "awk 'length > 120 {print}' README.md"});
    let ranged = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print}' README.md"});
    assert_eq!(parse_simple_exec_read_target(&whole), None);
    assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &whole), None);
    assert_eq!(
        repeated_file_read_family_key(tool_names::EXEC_COMMAND, &ranged),
        Some("unified_exec::read::README.md::off=65::lim=7".to_string())
    );
}

#[test]
fn awk_piped_measurement_still_counts_leading_read() {
    let args = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print}' README.md | cat"});
    let key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args);
    assert_eq!(key, Some("unified_exec::read::README.md::off=65::lim=7".to_string()));
}

#[test]
fn only_bare_sed_is_verbatim_for_positioned_evidence() {
    // Guards count pipes and awk via path/slice, but positioned evidence
    // needs the file's own lines: only a bare `sed -n` range print is
    // verbatim. Piped and awk outputs are transformed downstream.
    let bare = serde_json::json!({"cmd": "sed -n '65,71p' README.md"});
    assert!(parse_simple_exec_read_target(&bare).is_some_and(|target| target.verbatim));
    let piped = serde_json::json!({"cmd": "sed -n '65,71p' README.md | awk '{print n}'"});
    assert!(parse_simple_exec_read_target(&piped).is_some_and(|target| !target.verbatim));
    let awk = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print}' README.md"});
    assert!(parse_simple_exec_read_target(&awk).is_some_and(|target| !target.verbatim));
}

#[test]
fn awk_single_nr_reference_stays_untracked() {
    // One `NR` number cannot delimit a range: `NR>1` header-skips with
    // different bodies are diverse queries, not a retry loop. Grouping
    // them would trip the family cap on legitimate research.
    let first = serde_json::json!({"cmd": "awk 'NR>1 {print $1}' README.md"});
    let second = serde_json::json!({"cmd": "awk 'NR>1 {print $2}' README.md"});
    assert_eq!(parse_simple_exec_read_target(&first), None);
    assert_eq!(parse_simple_exec_read_target(&second), None);
    assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &first), None);
}

#[test]
fn sed_flag_like_path_stays_untracked() {
    let args = serde_json::json!({"cmd": "sed -n '1p' --help"});
    assert_eq!(parse_simple_exec_read_target(&args), None);
    assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args), None);
}

#[test]
fn awk_with_options_or_multiple_files_stays_untracked() {
    // Fail closed: options and multi-file scans are not simple reads.
    let flagged = serde_json::json!({"cmd": "awk -F: '{print $1}' README.md"});
    assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &flagged), None);
    let multi = serde_json::json!({"cmd": "awk '{print}' README.md CHANGELOG.md"});
    assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &multi), None);
}

#[test]
fn grep_measurement_probe_is_not_a_read_target() {
    // `grep -c` is search, not a positioned read: distinct queries are
    // diverse research (mirrors the code_search per-path exclusion), so it
    // must stay out of the read family/path caps.
    let args = serde_json::json!({"cmd": "grep -c '—' README.md"});
    assert_eq!(parse_simple_exec_read_target(&args), None);
    assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &args), None);
}

#[test]
fn repeated_read_path_counts_piped_sed_and_awk_measurements() {
    // Per-path total (not just family streak) must see the bypass shapes,
    // so 7+ README measurements trip the path cap even with mixed bodies.
    let piped_sed = serde_json::json!({"cmd": "sed -n '65,71p' README.md | awk '{print n}'"});
    let awk = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print}' README.md"});
    assert_eq!(repeated_read_path(tool_names::EXEC_COMMAND, &piped_sed), Some("README.md".to_string()));
    assert_eq!(repeated_read_path(tool_names::EXEC_COMMAND, &awk), Some("README.md".to_string()));
}

#[test]
fn spelling_variants_share_shell_family_and_path_count() {
    // `./` and interior `..` spellings are the same file: one family key
    // and one path bucket. A leading `..` escapes and stays distinct.
    let bare = serde_json::json!({"cmd": "sed -n '65,71p' README.md | awk '{print n}'"});
    let dotted = serde_json::json!({"cmd": "sed -n '65,71p' ./README.md | awk '{print n}'"});
    let escaping = serde_json::json!({"cmd": "awk 'NR>=65 && NR<=71 {print}' ../README.md"});
    let bare_key = repeated_file_read_family_key(tool_names::EXEC_COMMAND, &bare);
    assert_eq!(bare_key, Some("unified_exec::read::README.md::off=65::lim=7".to_string()));
    assert_eq!(repeated_file_read_family_key(tool_names::EXEC_COMMAND, &dotted), bare_key);
    assert_eq!(repeated_read_path(tool_names::EXEC_COMMAND, &dotted), Some("README.md".to_string()));
    assert_eq!(repeated_read_path(tool_names::EXEC_COMMAND, &escaping), Some("../README.md".to_string()));
}

#[test]
fn blocked_session_measurement_loop_trips_path_cap() {
    // Incident-shaped replay: session 20261008T094713Z burned 32 tool calls
    // on consecutive `README.md:65-71` table-width probes. The per-path cap
    // (`MAX_SAME_FILE_PATH_READ_CALLS`) must trip on this sequence even
    // though the bodies vary per call. `;`-compounds, `grep -c` searches,
    // and whole-file `length` scans stay untracked by design, so the hit
    // count is below the raw call count — but still above the cap.
    let probe_shapes = [
        r#"sed -n '65,71p' README.md | cat -A | head -20"#,
        r#"awk 'NR>=65 && NR<=71 {print NR": ["$0"]"}' README.md"#,
        r#"awk 'NR>=65 && NR<=71 {print NR": "$0}' README.md | awk '{print length($0), $0}'"#,
        r#"awk 'NR==67 || NR==68 || NR==69 || NR==70 || NR==71 {print}' README.md"#,
        r#"awk 'NR>=65 && NR<=71 {print}' README.md; sed -n '65p' README.md | wc -c"#,
        r#"awk 'NR>=65 && NR<=71 {n=length($0); print NR": len="n}' README.md"#,
        r#"awk 'NR>=65 && NR<=71 {n=length($0); print NR": c="n}' README.md"#,
        r#"sed -n '65,71p' README.md | awk '{n=length($0); print n}'"#,
        r#"grep -c 'x' README.md"#,
        r#"awk 'length > 120 {print}' README.md"#,
        r#"awk 'NR==67 {print $0} NR==68 {print $0}' README.md | cat"#,
        r#"sed -n '65,71p' README.md | awk '{n=length($0); print n}'"#,
    ];
    let hits = probe_shapes
        .iter()
        .filter(|command| {
            repeated_read_path(tool_names::EXEC_COMMAND, &serde_json::json!({"cmd": command}))
                == Some("README.md".to_string())
        })
        .count();
    assert!(
        hits > MAX_SAME_FILE_PATH_READ_CALLS,
        "incident loop must exceed the per-path cap: {hits} hits, cap {MAX_SAME_FILE_PATH_READ_CALLS}"
    );
}

#[test]
fn advertised_per_path_read_cap_matches_enforcement() {
    // Lockstep guard for a hand-synced mirror: enforcement lives here
    // (`MAX_SAME_FILE_PATH_READ_CALLS`) while the model-facing
    // advertisement lives in `vtcode-core` (`harness_limits.rs`
    // `MAX_SAME_FILE_PATH_READS`, a separate crate so the const cannot be
    // shared). Render the prompt section and require the advertised line
    // to carry the live enforcement value: drift on either side fails.
    let mut prompt = String::from("Base prompt");
    vtcode_core::prompts::upsert_harness_limits_section(&mut prompt, 600, 2);
    assert!(
        prompt.contains(&format!("max_same_file_path_reads: {MAX_SAME_FILE_PATH_READ_CALLS}")),
        "advertised per-path cap must match enforcement ({MAX_SAME_FILE_PATH_READ_CALLS}): {prompt}"
    );
}

#[test]
fn repeated_file_read_family_key_returns_none_for_missing_command() {
    let args = serde_json::json!({});
    let key = repeated_file_read_family_key(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(key, None);
}

#[test]
fn repeated_file_read_family_key_tracks_code_search_identity() {
    let args = serde_json::json!({"query": "HarnessTurnState", "path": "README.md"});
    let key = repeated_file_read_family_key(tool_names::CODE_SEARCH, &args)
        .expect("valid code_search should have a loop identity");
    assert!(key.starts_with("code_search::"));
    assert!(key.contains("HarnessTurnState"));
}

#[test]
fn repeated_read_path_excludes_code_search_distinct_queries() {
    // Distinct `code_search` queries scoped to one path are diverse
    // research, not pagination: the query-aware family cap guards
    // identical searches, so the per-path total must not count them.
    // This is the `benches/startup.rs` regression: seven distinct queries
    // tripped the per-turn cap and blocked planning with no draft.
    assert_eq!(
        repeated_read_path(tool_names::CODE_SEARCH, &serde_json::json!({"query": "fn", "path": "README.md"})),
        None
    );
    assert_eq!(repeated_read_path(tool_names::CODE_SEARCH, &serde_json::json!({"query": "fn"})), None);
    assert_eq!(
        repeated_read_path(tool_names::CODE_SEARCH, &serde_json::json!({"query": "fn", "path": "./docs/../README.md"})),
        None
    );
}

#[test]
fn code_search_family_cap_reports_path_not_serialized_identity() {
    let args = serde_json::json!({"query": "a very long query", "path": "./docs/../README.md"});
    let decision = check_read_family_cap(
        tool_names::CODE_SEARCH,
        &args,
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        false,
    );

    let ReadFamilyCapDecision::Tripped { target, block_reason, .. } = decision else {
        panic!("expected code_search family cap to trip");
    };
    assert_eq!(target, "README.md");
    assert!(block_reason.contains("'README.md'"));
    assert!(!block_reason.contains("a very long query"));
}

#[test]
fn read_family_cap_decision_below_cap_for_non_read_tool() {
    let decision = check_read_family_cap(
        tool_names::UNIFIED_EXEC,
        &serde_json::json!({"command": "ls -la"}),
        99,
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        false,
    );
    assert_eq!(decision, ReadFamilyCapDecision::BelowCap);
}

#[test]
fn read_family_cap_decision_below_cap_when_streak_under_cap() {
    let decision = check_read_family_cap(
        tool_names::UNIFIED_FILE,
        &serde_json::json!({"action": "read", "path": "src/lib.rs", "offset": 0, "limit": 100}),
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS - 1,
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        false,
    );
    assert_eq!(decision, ReadFamilyCapDecision::BelowCap);
}

#[test]
fn read_family_cap_decision_tripped_at_cap() {
    let decision = check_read_family_cap(
        tool_names::UNIFIED_FILE,
        &serde_json::json!({"action": "read", "path": "src/lib.rs", "offset": 0, "limit": 100}),
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        false,
    );
    match decision {
        ReadFamilyCapDecision::Tripped { target, block_reason, error_content } => {
            assert_eq!(target, "src/lib.rs");
            assert!(block_reason.contains("per-turn family cap"));
            assert!(error_content.contains("repeated_read_family"));
        }
        ReadFamilyCapDecision::BelowCap => panic!("expected Tripped at cap"),
    }
}

#[test]
fn read_family_cap_planning_guidance_directs_plan_synthesis() {
    let decision = check_read_family_cap(
        tool_names::UNIFIED_FILE,
        &serde_json::json!({"action": "read", "path": "src/lib.rs"}),
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        true,
    );
    match decision {
        ReadFamilyCapDecision::Tripped { block_reason, error_content, .. } => {
            assert!(block_reason.contains("<proposed_plan>"));
            assert!(error_content.contains("<proposed_plan>"));
        }
        ReadFamilyCapDecision::BelowCap => panic!("expected Tripped at cap"),
    }
}

#[test]
fn read_family_target_strips_slice_suffix() {
    assert_eq!(read_family_target("unified_file::read::src/cli/update.rs::off=81::lim=229"), "src/cli/update.rs");
    assert_eq!(read_family_target("read_file::src/main.rs::off=80::lim=200::raw=true"), "src/main.rs");
    assert_eq!(read_family_target("unified_file::read::src/cli/update.rs"), "src/cli/update.rs");
    assert_eq!(read_family_target("unified_exec::run::cat README.md"), "cat README.md");
    assert_eq!(read_family_target("unified_exec::read::Cargo.toml::off=440::lim=81"), "Cargo.toml");
    assert_eq!(read_family_target("read_file::src/lib.rs"), "src/lib.rs");
}

#[test]
fn read_family_cap_decision_tripped_above_cap() {
    let decision = check_read_family_cap(
        tool_names::READ_FILE,
        &serde_json::json!({"path": "src/main.rs"}),
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS + 5,
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        false,
    );
    assert!(matches!(decision, ReadFamilyCapDecision::Tripped { .. }));
}

#[test]
fn read_family_cap_decision_uses_bare_path_target_when_unpaginated() {
    let decision = check_read_family_cap(
        tool_names::UNIFIED_FILE,
        &serde_json::json!({"action": "read", "path": "src/cli/update.rs"}),
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        false,
    );
    match decision {
        ReadFamilyCapDecision::Tripped { target, .. } => {
            assert_eq!(target, "src/cli/update.rs");
        }
        ReadFamilyCapDecision::BelowCap => panic!("expected Tripped at cap"),
    }
}

#[test]
fn is_read_action_returns_true_for_unified_file_read() {
    assert!(is_read_action(
        tool_names::UNIFIED_FILE,
        &serde_json::json!({"action": "read", "path": "src/lib.rs"})
    ));
    assert!(is_read_action(tool_names::UNIFIED_FILE, &serde_json::json!({"path": "src/lib.rs"})));
    assert!(!is_read_action(
        tool_names::UNIFIED_FILE,
        &serde_json::json!({"action": "write", "path": "src/lib.rs"})
    ));
}

#[test]
fn extract_read_path_returns_path_from_args() {
    assert_eq!(extract_read_path(&serde_json::json!({"path": "src/lib.rs"})), Some("src/lib.rs".to_string()));
    assert_eq!(extract_read_path(&serde_json::json!({})), None);
}

#[test]
fn max_same_file_path_read_calls_is_stricter_than_family_cap() {
    const _: () = assert!(
        MAX_SAME_FILE_PATH_READ_CALLS >= MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS,
        "per-file-path cap must be >= family cap"
    );
    const _: () = assert!(MAX_SAME_FILE_PATH_READ_CALLS < 10, "per-file-path cap must catch excessive reads");
}

#[test]
fn planning_doubles_both_read_caps() {
    assert_eq!(effective_read_family_cap(false), MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS);
    assert_eq!(effective_read_path_cap(false), MAX_SAME_FILE_PATH_READ_CALLS);
    assert_eq!(effective_read_family_cap(true), MAX_CONSECUTIVE_SAME_FILE_READ_FAMILY_CALLS.saturating_mul(2));
    assert_eq!(effective_read_path_cap(true), MAX_SAME_FILE_PATH_READ_CALLS.saturating_mul(2));
}

#[test]
fn plan_artifact_read_is_scoped_to_runtime_owned_paths() {
    let plan_file = serde_json::json!({"action": "read", "path": ".vtcode/plans/session-123.md"});
    assert!(is_plan_artifact_read(tool_names::UNIFIED_FILE, &plan_file).is_some());

    let tracker = serde_json::json!({"action": "read", "path": ".vtcode/tasks/current_task.md"});
    assert!(is_plan_artifact_read(tool_names::UNIFIED_FILE, &tracker).is_some());

    // Ordinary markdown docs must not receive plan-specific guidance.
    let doc = serde_json::json!({"action": "read", "path": "docs/guides/planning-workflow.md"});
    assert!(is_plan_artifact_read(tool_names::UNIFIED_FILE, &doc).is_none());

    // Paths that merely contain `plan` are not plan artifacts.
    let code = serde_json::json!({"action": "read", "path": "src/agent/runloop/unified/planning_workflow_state.rs"});
    assert!(is_plan_artifact_read(tool_names::UNIFIED_FILE, &code).is_none());
}
