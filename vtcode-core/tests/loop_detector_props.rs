use proptest::prelude::*;
use serde_json::json;
use vtcode_core::core::loop_detector::LoopDetector;

// Property 1: Loop detection prevents infinite repetition (Req 1.1)
proptest! {
    #[test]
    fn prop_detects_repetition_by_third_call(tool_name in "[a-zA-Z0-9_]{3,16}", path in ".{0,40}") {
        let mut detector = LoopDetector::new();
        let args = json!({ "path": path });

        // First two calls should not trigger a stop
        prop_assert!(detector.record_call(&tool_name, &args).is_none());
        prop_assert!(detector.record_call(&tool_name, &args).is_none());

        // Third identical call must trigger a halt warning
        let warning = detector.record_call(&tool_name, &args);
        prop_assert!(warning.is_some());
        prop_assert!(warning.unwrap().contains("HARD STOP"));
    }
}

// Property 2: Root path normalization consistency (Req 1.2)
proptest! {
    #[test]
    fn prop_root_variations_normalize_to_same_signature(
        paths in prop::collection::vec(prop::sample::select(vec!["", ".", "./", "././", "/", "//"]), 3)
    ) {
        let mut detector = LoopDetector::new();

        // First two root variations should not warn
        prop_assert!(
            detector.record_call("list_files", &json!({ "path": paths[0] })).is_none(),
            "first root variation should not warn"
        );
        prop_assert!(
            detector.record_call("list_files", &json!({ "path": paths[1] })).is_none(),
            "second root variation should not warn"
        );

        // Third variation should still count as the same signature and trigger loop detection
        let warning = detector.record_call("list_files", &json!({ "path": paths[2] }));
        prop_assert!(warning.is_some());
    }
}


