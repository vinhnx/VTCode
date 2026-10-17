use super::*;
use vtcode_core::llm::provider::FinishReason;

#[test]
fn copilot_finish_reason_maps_protocol_values() {
    assert_eq!(map_copilot_finish_reason("end_turn"), FinishReason::Stop);
    assert_eq!(map_copilot_finish_reason("max_tokens"), FinishReason::Length);
    assert_eq!(map_copilot_finish_reason("length"), FinishReason::Length);
    assert_eq!(map_copilot_finish_reason("refusal"), FinishReason::Refusal);
    assert_eq!(map_copilot_finish_reason("cancelled"), FinishReason::Error("cancelled".to_string()));
}

#[test]
fn copilot_reasoning_delta_normalizes_chunk_boundaries() {
    assert_eq!(
        normalize_copilot_reasoning_delta(
            "The user wants me to run `cargo check` and report what I see.",
            "Running cargo check".to_string()
        ),
        " Running cargo check"
    );
    assert_eq!(normalize_copilot_reasoning_delta("prefix\n", "next".to_string()), "next");
}

#[test]
fn copilot_reasoning_delta_collapses_single_newlines_inside_chunk() {
    assert_eq!(
        normalize_copilot_reasoning_delta(
            "Run",
            " cargo fmt and report the\n results\n.\nRunning cargo fmt".to_string()
        ),
        " cargo fmt and report the results. Running cargo fmt"
    );
}

#[test]
fn reasoning_preserves_unicode_paragraphs_and_closing_punctuation() {
    assert_eq!(
        normalize_copilot_reasoning_delta("", "Xin\nchào\n\nViệt Nam\n".to_string()),
        "Xin chào\n\nViệt Nam\n"
    );
    for punctuation in [".", ",", ";", ":", "!", "?", ")", "]", "}"] {
        assert_eq!(normalize_copilot_reasoning_delta("đã xong", punctuation.to_string()), punctuation);
    }
    assert_eq!(normalize_copilot_reasoning_delta("prefix ", "next".to_string()), "next");
    assert_eq!(normalize_copilot_reasoning_delta("prefix", " next".to_string()), " next");
}

#[test]
fn asymmetric_reasoning_chunks_accumulate_without_losing_paragraph_boundaries() {
    let mut reasoning = String::new();
    let mut deltas = Vec::new();
    for chunk in ["Tiếng", "Việt\nNam", ".", "\n\n", "Kế\ntiếp"] {
        let delta = normalize_copilot_reasoning_delta(&reasoning, chunk.to_string());
        reasoning.push_str(&delta);
        deltas.push(delta);
    }
    assert_eq!(deltas, ["Tiếng", " Việt Nam", ".", "\n\n", "Kế tiếp"]);
    assert_eq!(reasoning, "Tiếng Việt Nam.\n\nKế tiếp");
}

#[test]
fn finish_reason_trims_protocol_values_and_retains_unknown_reasons() {
    assert_eq!(map_copilot_finish_reason("  end_turn\n"), FinishReason::Stop);
    assert_eq!(map_copilot_finish_reason(" length "), FinishReason::Length);
    assert_eq!(map_copilot_finish_reason(" custom-stop "), FinishReason::Error("custom-stop".to_string()));
    assert_eq!(map_copilot_finish_reason(" \n"), FinishReason::Error(String::new()));
}
