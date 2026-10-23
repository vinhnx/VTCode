use super::*;

#[tokio::test]
async fn catalog_only_refresh_preserves_frozen_envelope_but_new_instructions_replace_it() {
    let temp = TempDir::new().unwrap();
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "prompt-refresh")).await;
    let mut bundle = runner.build_validated_runtime_prompt_bundle(false).await.unwrap();
    let original_prompt = bundle.request_envelope.system_prompt();
    let original_tools = bundle.request_envelope.ordered_tools();
    let original_segment = bundle.request_envelope.segment_id().to_owned();
    let original_digest = bundle.request_envelope.instruction_digest();
    let original_version = bundle.tool_snapshot.version;

    runner
        .tool_registry
        .tool_catalog_state()
        .note_explicit_refresh("unchanged test catalog");
    let changed = runner
        .refresh_runtime_prompt_bundle_if_catalog_changed(&mut bundle, false)
        .await
        .unwrap();

    assert!(!changed, "catalog bookkeeping alone must not replace the provider prefix");
    assert!(bundle.tool_snapshot.version > original_version);
    assert!(Arc::ptr_eq(&original_prompt, &bundle.request_envelope.system_prompt()));
    assert!(Arc::ptr_eq(&original_tools, &bundle.request_envelope.ordered_tools()));
    assert_eq!(bundle.request_envelope.segment_id(), original_segment);
    assert_eq!(bundle.request_envelope.instruction_digest(), original_digest);

    let instruction = "Distinct task instruction for the next request segment.";
    assert!(!original_prompt.contains(instruction));
    // Start a stable instruction section rather than extending a dynamic
    // appendix, which the existing prefix identity deliberately excludes.
    runner.system_prompt.push_str("\n\n## Task-specific instructions\n");
    runner.system_prompt.push_str(instruction);
    runner
        .tool_registry
        .tool_catalog_state()
        .note_explicit_refresh("new test instructions");
    let changed = runner
        .refresh_runtime_prompt_bundle_if_catalog_changed(&mut bundle, false)
        .await
        .unwrap();

    assert!(changed, "a provider-visible instruction change must replace the prefix");
    assert!(bundle.request_envelope.system_prompt().contains(instruction));
    assert!(!Arc::ptr_eq(&original_prompt, &bundle.request_envelope.system_prompt()));
    assert_ne!(bundle.request_envelope.instruction_digest(), original_digest);
    assert_ne!(bundle.request_envelope.segment_id(), original_segment);
}
