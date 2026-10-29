use super::*;

#[test]
fn capability_change_starts_segment_without_mutating_frozen_prompt() {
    let mut stats = SessionStats::default();
    let first = stats.request_envelope_shared("model", "provider", "build", "fixed".into(), None, 7, 11);
    let same = stats.request_envelope_shared("model", "provider", "build", "fixed".into(), None, 7, 11);
    assert_eq!(first.segment_id(), same.segment_id());
    let changed = stats.request_envelope_shared("model", "provider", "build", "fixed".into(), None, 7, 12);
    assert_ne!(first.segment_id(), changed.segment_id());
    assert_ne!(first.prefix_hash(), changed.prefix_hash());
    assert_eq!(first.system_prompt(), changed.system_prompt());
}
#[test]
fn runtime_suffix_refreshes_without_invalidating_stable_segment() {
    let mut stats = SessionStats::default();
    let first = stats.request_envelope_shared(
        "model",
        "provider",
        "build",
        "fixed\n[Runtime Context]\n- turn: 1".into(),
        None,
        7,
        11,
    );
    let second = stats.request_envelope_shared(
        "model",
        "provider",
        "build",
        "fixed\n[Runtime Context]\n- turn: 2".into(),
        None,
        7,
        11,
    );

    assert_eq!(first.segment_id(), second.segment_id());
    assert_eq!(first.prefix_hash(), second.prefix_hash());
    assert_ne!(first.system_prompt(), second.system_prompt());
}
#[test]
fn equivalent_requests_reuse_the_same_segment_bytes() {
    let mut stats = SessionStats::default();
    let first = stats.request_envelope(
        "model",
        "provider",
        "build",
        "fixed".to_string(),
        vec![function_tool("zeta"), function_tool("exec_command")],
        7,
    );
    let second = stats.request_envelope(
        "model",
        "provider",
        "build",
        "fixed".to_string(),
        vec![function_tool("exec_command"), function_tool("zeta")],
        7,
    );

    assert_eq!(first.segment_id(), second.segment_id());
    assert_eq!(first.system_prompt().as_bytes(), second.system_prompt().as_bytes());
    assert_eq!(first.catalog_hash(), second.catalog_hash());
}
#[test]
fn shared_request_envelope_reuses_catalog_until_segment_boundary() {
    let mut stats = SessionStats::default();
    let tools = Arc::new(vec![function_tool("exec_command"), function_tool("zeta")]);

    let first = stats.request_envelope_shared(
        "model",
        "provider",
        "build",
        "fixed".to_string(),
        Some(Arc::clone(&tools)),
        7,
        11,
    );
    let second = stats.request_envelope_shared(
        "model",
        "provider",
        "build",
        "fixed".to_string(),
        Some(Arc::clone(&tools)),
        7,
        11,
    );

    assert_eq!(first.segment_id(), second.segment_id());
    assert!(Arc::ptr_eq(stats.request_envelope_source_tools.as_ref().expect("source catalog"), &tools));

    let transition = stats.begin_request_segment(SegmentBoundaryReason::Compaction);
    assert_eq!(transition.previous_segment_id.as_deref(), Some(first.segment_id()));
    assert!(stats.request_envelope_source_tools.is_none());

    let third = stats.request_envelope_shared(
        "model",
        "provider",
        "build",
        "fixed".to_string(),
        Some(Arc::clone(&tools)),
        7,
        11,
    );
    assert_ne!(first.segment_id(), third.segment_id());
}
#[test]
fn shared_request_envelope_preserves_equivalent_distinct_catalogs() {
    let mut stats = SessionStats::default();
    let first_tools = Arc::new(vec![function_tool("zeta"), function_tool("exec_command")]);
    let second_tools = Arc::new(vec![function_tool("exec_command"), function_tool("zeta")]);

    let first =
        stats.request_envelope_shared("model", "provider", "build", "fixed".to_string(), Some(first_tools), 7, 11);
    let second =
        stats.request_envelope_shared("model", "provider", "build", "fixed".to_string(), Some(second_tools), 7, 11);

    assert_eq!(first.segment_id(), second.segment_id());
    assert_eq!(first.catalog_hash(), second.catalog_hash());
    assert_eq!(first.ordered_tools(), second.ordered_tools());
}
#[test]
fn compaction_reserves_exactly_one_new_segment() {
    let mut stats = SessionStats::default();
    let first = stats.request_envelope("model", "provider", "build", "fixed".to_string(), vec![], 7);
    let before_bytes = serde_json::to_vec(&(
        first.system_prompt().to_string(),
        first.ordered_tools().as_ref().clone(),
        first.instruction_digest(),
    ))
    .expect("serialize request envelope");
    let transition = stats.begin_request_segment(SegmentBoundaryReason::Compaction);
    let second = stats.request_envelope("model", "provider", "build", "fixed".to_string(), vec![], 7);
    let after_bytes = serde_json::to_vec(&(
        first.system_prompt().to_string(),
        first.ordered_tools().as_ref().clone(),
        first.instruction_digest(),
    ))
    .expect("serialize request envelope");

    assert_eq!(transition.boundary_reason, SegmentBoundaryReason::Compaction);
    assert_eq!(transition.previous_segment_id.as_deref(), Some(first.segment_id()));
    assert_eq!(transition.new_segment_id, second.segment_id());
    assert_ne!(first.segment_id(), second.segment_id());
    assert_eq!(before_bytes, after_bytes);
}
