use super::*;

#[tokio::test]
async fn path_cap_shares_counter_across_spellings_and_resets_on_write() {
    let mut backing = TestContextBacking::new().await;
    let sample_path = backing.sample_file.to_string_lossy().to_string();
    let mut ctx = backing.turn_processing_context();

    // Alternate absolute and workspace-relative spellings of the same file
    // (with distinct slices so the identical-slice family guard stays out):
    // all six count toward ONE normalized counter.
    for idx in 0..6 {
        let spelling = if idx % 2 == 0 {
            sample_path.clone()
        } else {
            "sample.txt".to_string()
        };
        let args = json!({"path": spelling, "offset": idx + 1, "limit": 1});
        let outcome =
            enforce_repeated_read_only_call_guard(&mut ctx, &format!("read_{idx}"), tool_names::READ_FILE, &args, true);
        assert!(outcome.is_none(), "read {idx} must pass (shared counter at {})", idx + 1);
    }

    // Seventh read trips the shared cap; the message keeps the raw spelling
    // the model used, matching the read-after-write guard's display choice.
    let tripped = enforce_repeated_read_only_call_guard(
        &mut ctx,
        "read_tripped",
        tool_names::READ_FILE,
        &json!({"path": "sample.txt", "offset": 7, "limit": 1}),
        true,
    );
    assert!(matches!(tripped, Some(ValidationResult::ReadCapBlocked)));
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.content.as_text().contains("Repeated reads of 'sample.txt'")),
        "block message must show the spelling the model used"
    );

    // A successful write resets this path's counter: post-write reads are
    // verification of new content, not pagination.
    ctx.harness_state.record_written_file(&sample_path);
    let after_write = enforce_repeated_read_only_call_guard(
        &mut ctx,
        "read_after_write",
        tool_names::READ_FILE,
        &json!({"path": sample_path, "offset": 1, "limit": 1}),
        true,
    );
    assert!(after_write.is_none(), "post-write verification read must pass");
}
