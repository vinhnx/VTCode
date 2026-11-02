use super::*;

#[test]
fn exec_slice_read_reuses_identical_slice_cross_turn() {
    let history = ToolExecutionHistory::new(10);

    // Session evidence (20261015T232419): `sed -n '270,300p' README.md` was
    // re-emitted 148s later and re-executed because no reuse layer accepted
    // exec tools. The recorded args carry alias materialization, the query
    // re-emits the raw provider spelling — identity must bridge both.
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"sed -n '270,300p' README.md","command":"sed -n '270,300p' README.md"}),
        json!({"output":"line 270\nline 271\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let query = json!({"cmd":"sed -n '270,300p' README.md"});
    let served = history.find_recent_successful_by_read_target(tools::EXEC_COMMAND, &query, Duration::from_secs(600));
    assert!(served.is_some(), "identical slice must reuse within TTL");
}
#[test]
fn exec_slice_read_rejects_other_slices_and_transforms() {
    let history = ToolExecutionHistory::new(10);
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"sed -n '270,300p' README.md","command":"sed -n '270,300p' README.md"}),
        json!({"output":"line 270\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // Different slice: no coverage proof exists for command reads.
    let other_slice = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -n '280,300p' README.md"}),
        Duration::from_secs(600),
    );
    assert!(other_slice.is_none(), "different slice must not be served");

    // Piped transform differs in transform class, and must never be served
    // from a verbatim record.
    let piped = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -n '270,300p' README.md | awk '{print length}'"}),
        Duration::from_secs(600),
    );
    assert!(piped.is_none(), "verbatim record cannot serve a piped query");

    // Mutating command: fail closed regardless of shape.
    let mutating = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -i '270,300s/a/b/' README.md"}),
        Duration::from_secs(600),
    );
    assert!(mutating.is_none(), "mutating commands are never eligible");

    // Non-command shapes (`cat extra1 extra2`) never parse as slice reads.
    let multi = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"cat README.md"}),
        Duration::from_secs(600),
    );
    assert!(multi.is_none());
}
#[test]
fn exec_slice_read_rejects_expired_ttl_and_fresh_after_mutation() {
    let history = ToolExecutionHistory::new(10);
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"sed -n '1,5p' README.md","command":"sed -n '1,5p' README.md"}),
        json!({"output":"top\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // TTL expired: no reuse.
    let expired = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -n '1,5p' README.md"}),
        Duration::from_secs(0),
    );
    assert!(expired.is_none(), "TTL still bounds exec-slice reuse");

    // A later success record for a *different* slice stays invisible to the
    // original query but does not poison it either.
    history.add_record(ToolExecutionRecord::success(
        tools::WRITE_FILE.to_string(),
        tools::WRITE_FILE.to_string(),
        false,
        None,
        json!({"path":"README.md","content":"mutated"}),
        json!({"success":true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));
    let after_mutation = history.find_recent_successful_by_read_target(
        tools::EXEC_COMMAND,
        &json!({"cmd":"sed -n '1,5p' README.md"}),
        Duration::from_secs(600),
    );
    // NOTE: replay scanning skips records whose tool name differs, so the
    // write does not invalidate here — invalidation is the caller's job
    // (`invalidate_for_path`). This documents that expectation explicitly.
    assert!(after_mutation.is_some(), "mutating records of other tools are skipped, not scanned");
}
#[test]
fn mutation_invalidates_cached_exec_slice_read_of_same_path() {
    let history = ToolExecutionHistory::new(10);
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"sed -n '1,5p' README.md","command":"sed -n '1,5p' README.md"}),
        json!({"output":"stale\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    // A write to the same path must drop the exec-slice record; otherwise the
    // cross-turn cache would serve stale pre-mutation content.
    history.invalidate_for_path("README.md");
    assert!(
        history
            .find_recent_successful_by_read_target(
                tools::EXEC_COMMAND,
                &json!({"cmd":"sed -n '1,5p' README.md"}),
                Duration::from_secs(600),
            )
            .is_none(),
        "a mutation of the read path must invalidate the cached exec slice"
    );
}
#[test]
fn invalidate_all_reads_drops_exec_slice_records() {
    let history = ToolExecutionHistory::new(10);
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"sed -n '1,5p' README.md","command":"sed -n '1,5p' README.md"}),
        json!({"output":"stale\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));
    // A non-slice command must survive (not a read record).
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"action":"run","cmd":"cargo build","command":"cargo build"}),
        json!({"output":"built\n","exit_code":0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    history.invalidate_all_reads();

    assert!(
        history
            .find_recent_successful_by_read_target(
                tools::EXEC_COMMAND,
                &json!({"cmd":"sed -n '1,5p' README.md"}),
                Duration::from_secs(600),
            )
            .is_none(),
        "pathless mutation must drop cached exec slices too"
    );
}
