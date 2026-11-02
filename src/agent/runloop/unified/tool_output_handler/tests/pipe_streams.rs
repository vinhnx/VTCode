use super::*;

#[test]
fn ordered_stream_texts_deduplicates_merged_output_aliases() {
    let output = serde_json::json!({
        "output": "stdout line\nstderr line",
        "stdout": "stdout line",
        "stderr": "stderr line"
    });

    assert_eq!(ordered_stream_texts(&output), vec!["stdout line\nstderr line"]);
}
#[test]
fn ordered_stream_texts_preserves_distinct_pipe_streams() {
    let output = serde_json::json!({
        "output": "merged line",
        "stdout": "stdout line",
        "stderr": "stderr line"
    });

    assert_eq!(ordered_stream_texts(&output), vec!["merged line", "stdout line", "stderr line"]);
}
#[test]
fn canonical_pipe_streams_preserve_unrepresented_content() {
    let output = serde_json::json!({
        "stdout": "command output",
        "content": "additional structured content"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![
            (Some("stdout"), "command output"),
            (None, "additional structured content")
        ]
    );
}
#[test]
fn canonical_pipe_streams_keep_merged_output_once() {
    let output = serde_json::json!({
        "output": "stdout line\nstderr line",
        "stdout": "stdout line",
        "stderr": "stderr line"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].label, None);
    assert_eq!(streams[0].text, "stdout line\nstderr line");
}
#[test]
fn canonical_pipe_streams_label_separate_streams() {
    let output = serde_json::json!({
        "stdout": "stdout line",
        "stderr": "stderr line"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![(Some("stdout"), "stdout line"), (Some("stderr"), "stderr line")]
    );
}
#[test]
fn canonical_pipe_streams_preserve_identical_named_streams() {
    let output = serde_json::json!({
        "stdout": "same output",
        "stderr": "same output"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![(Some("stdout"), "same output"), (Some("stderr"), "same output")]
    );
}
#[test]
fn canonical_pipe_streams_require_distinct_merged_occurrences() {
    let single_occurrence = serde_json::json!({
        "output": "same output",
        "stdout": "same output",
        "stderr": "same output"
    });
    let streams = canonical_pipe_streams(&single_occurrence);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![(Some("stdout"), "same output"), (Some("stderr"), "same output")]
    );
    assert_eq!(stderr_for_inline_display(&single_occurrence), Some("same output"));

    let distinct_occurrences = serde_json::json!({
        "output": "same output\nsame output",
        "stdout": "same output",
        "stderr": "same output"
    });
    let streams = canonical_pipe_streams(&distinct_occurrences);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].label, None);
    assert_eq!(streams[0].text, "same output\nsame output");
    assert_eq!(stderr_for_inline_display(&distinct_occurrences), None);
}
#[test]
fn canonical_pipe_streams_preserve_merged_lines_when_named_streams_overlap() {
    let output = serde_json::json!({
        "output": "same output\nmerged-only output",
        "stdout": "same output",
        "stderr": "same output"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![
            (Some("stdout"), "same output"),
            (Some("stderr"), "same output"),
            (None, "same output\nmerged-only output")
        ]
    );
}
#[test]
fn canonical_pipe_streams_preserve_full_named_alias() {
    let output = serde_json::json!({
        "output": "stdout line",
        "stdout": "stdout line\nsecond stdout line",
        "stderr": "stderr line"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![
            (Some("stdout"), "stdout line\nsecond stdout line"),
            (Some("stderr"), "stderr line")
        ]
    );
}
#[test]
fn canonical_pipe_streams_preserve_distinct_streams_when_output_is_preview() {
    let output = serde_json::json!({
        "output": "preview",
        "stdout": "preview\nstdout line",
        "stderr": "preview\nstderr line"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![
            (Some("stdout"), "preview\nstdout line"),
            (Some("stderr"), "preview\nstderr line")
        ]
    );
}
#[test]
fn normalize_terminal_output_lines_handles_ansi_rewrites_and_blanks() {
    let capture = "stale\n\x1b[2J\x1b[H\x1b[31mred\x1b[0m\rfinal\n\nlast\n";

    assert_eq!(normalize_terminal_output_lines(capture), vec!["final", "", "last"]);
    assert_eq!(normalize_terminal_output_lines("abc\x08d\n"), vec!["abd"]);
}
